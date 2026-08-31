//! Verdict generation.
//!
//! Builds a prompt from the ranked matches and asks an LLM backend (Ollama or
//! any OpenAI-compatible API) for a scoped verdict. The prompt **forbids claiming
//! non-existence**: results are always phrased as "found in the sources checked",
//! and a clean result means "keep looking before committing", never a green light.

use crate::llm::Llm;
use crate::model::{Match, Query, Saturation, Source, Verdict};

/// The fixed humble caveat shown on every verdict. Never weaken this.
pub const CAVEAT: &str = "Not proof it doesn't exist — only that nothing close turned up \
in the sources checked. Keep looking (web, app stores, niche communities) before committing.";

/// Render the list of sources actually searched, for the prompt.
fn source_list(sources_checked: &[Source]) -> String {
    if sources_checked.is_empty() {
        return "the selected open-source registries".to_string();
    }
    sources_checked
        .iter()
        .map(|s| s.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Build the LLM prompt enforcing the integrity rules.
///
/// `sources_checked` must be the sources that actually responded — the prompt
/// only ever tells the model about coverage that really happened, so the model
/// can't be steered into claiming a source was searched when it wasn't.
pub fn build_prompt(query: &Query, matches: &[Match], sources_checked: &[Source]) -> String {
    let mut prompt = String::new();

    prompt.push_str(&format!(
        "You are a skeptical prior-art analyst for SOFTWARE DEVELOPER TOOLS ONLY. Your \
         default assumption is that the idea has already been built — lean toward Crowded \
         or Saturated when in doubt. The user has an idea for a dev tool and we searched \
         these open-source sources for existing implementations: {}.\n\n",
        source_list(sources_checked),
    ));

    prompt.push_str(&format!("## Idea\n{}\n\n", query.idea));

    if matches.is_empty() {
        prompt.push_str("## Matches\nNo matches were found in the sources checked.\n\n");
    } else {
        let top10: Vec<&Match> = matches.iter().take(10).collect();
        let avg_sim: f32 = top10.iter().map(|m| m.similarity).sum::<f32>() / top10.len() as f32;

        prompt.push_str("## Matches found (ranked by cosine similarity to the idea)\n");
        prompt.push_str(&format!(
            "Top-10 average similarity: {:.2} (scale: 0.0 = unrelated, 0.5 = tangential, \
             0.7+ = strong match). Each match also shows a popularity figure (a per-source \
             signal of how established it is — stars, downloads, etc.) and its URL; a strong \
             match that is also popular is firmer prior art. Where the source reports one, a \
             match also shows when it was last updated. Treat that as colour on the prior art, \
             never as a reason to discount it: a tool that exists but looks unmaintained is \
             still proof the idea has been built, and may be worth mentioning as a space with \
             room for a maintained alternative. Matches with no date shown are ones whose \
             source does not publish one — that is not evidence of staleness.\n\n",
            avg_sim,
        ));
        let now = crate::freshness::now();
        for m in matches.iter().take(15) {
            let pop = match m.popularity {
                Some(p) => format!(", popularity {p}"),
                None => String::new(),
            };
            let url = if m.url.trim().is_empty() {
                String::new()
            } else {
                format!(" — {}", m.url)
            };
            let updated = m
                .last_updated
                .as_deref()
                .and_then(|ts| crate::freshness::age(ts, now))
                .map(|a| format!(", updated {}", a.label))
                .unwrap_or_default();
            prompt.push_str(&format!(
                "- **{}** ({}, sim {:.2}{}{}){}: {}\n",
                m.name, m.source, m.similarity, pop, updated, url, m.description,
            ));
        }
        if matches.len() > 15 {
            prompt.push_str(&format!(
                "- … and {} more with lower similarity\n",
                matches.len() - 15
            ));
        }
        prompt.push('\n');
    }

    prompt.push_str(
        "## Rules — you MUST follow these\n\
         - You can prove something EXISTS; you must NEVER claim something does not exist.\n\
         - All conclusions must be scoped to \"found in the sources checked\".\n\
         - Do not say \"this doesn't exist\" or \"there is no prior art\" — only that \
           nothing close turned up in the sources checked.\n\
         - If the idea is NOT about software, developer tools, or programming, respond \
           with level \"Open\" and headline \"This does not appear to be a software tool \
           idea — patent searches developer tool registries only.\"\n\
         - Focus ONLY on matches that directly address the SPECIFIC feature described in \
           the idea. Generic or tangential tools (e.g. a generic linter when the idea is \
           a specific kind of linter) do NOT count as prior art.\n\n",
    );

    prompt.push_str(
        "## How to choose the level\n\
         Use the similarity scores — they measure how closely each match relates to the idea:\n\
         - **Open**: no match has similarity >= 0.55, OR matches are only tangentially \
           related (they share a category but not the specific feature).\n\
         - **Crowded**: at least 2-3 matches with similarity >= 0.55 that directly \
           address the same problem.\n\
         - **Saturated**: 5+ strong matches (>= 0.60) covering the idea with little room \
           for differentiation.\n\n",
    );

    prompt.push_str(
        "## Output\n\
         Respond with ONLY a JSON object (no markdown fences, no commentary):\n\
         ```\n\
         {\n  \
           \"level\": \"Open\" | \"Crowded\" | \"Saturated\",\n  \
           \"headline\": \"one-sentence summary scoped to sources checked\",\n  \
           \"gaps\": [\"gap the user could fill\", ...]\n\
         }\n\
         ```\n\
         The headline MUST describe the user's idea and its closest matches above \
         — never an unrelated tool from the list — and must be scoped to the \
         sources checked. Never claim the idea does not exist or has no prior art.\n",
    );

    prompt
}

/// Phrases that assert non-existence. The integrity rule forbids ever telling a
/// user their idea doesn't exist (we only searched some sources), so if the
/// model emits one of these despite the prompt, we replace the text.
///
/// This is a deliberately broad, conservative backstop: a false positive only
/// downgrades the copy to a safe scoped headline, whereas a false negative is
/// an integrity violation — so we err toward catching more.
const ABSENCE_PHRASES: &[&str] = &[
    "does not exist",
    "doesn't exist",
    "do not exist",
    "don't exist",
    "no prior art",
    "nothing exists",
    "nothing like this",
    "never been built",
    "never been made",
    "never been implemented",
    "has not been built",
    "hasn't been built",
    "has not been implemented",
    "hasn't been implemented",
    "not been implemented",
    "no one has built",
    "no one has made",
    "no one else",
    "nobody else",
    "no one is doing",
    "no such tool",
    "no existing tool",
    "no existing solution",
    "no existing implementation",
    "no similar tool",
    "no similar project",
    "no comparable",
    "no competitors",
    "no alternatives",
    "no equivalent",
    "there is no tool",
    "there are no tools",
    "there is no existing",
    "there is no software",
    "there is no prior",
    "completely novel",
    "entirely new",
    "brand new concept",
    "first of its kind",
    "unprecedented",
    "greenfield",
    "wide open",
    "untapped",
    "gap in the market",
    "we found zero",
    "zero implementations",
    "zero results",
    "no one is solving",
    // Maintenance-flavoured absence. Once matches carry a last-updated date the
    // model can assert absence in a second way — not "nothing exists" but
    // "nothing *maintained* exists" — and that is the same unprovable claim:
    // only 9 of the 19 sources publish a date at all, so an undated match is
    // not an unmaintained one. These are the phrasings that generalise beyond
    // what was searched; saying a *specific* match is unmaintained ("the
    // closest match is no longer maintained") stays legal, because that is
    // read off the data we actually showed.
    "no maintained",
    "no actively developed",
    "nothing maintained",
    "no up-to-date",
];

/// Adverbs a model slips between a negation and its noun ("no **actively**
/// maintained tool"), which would otherwise walk an absence claim straight past
/// a literal substring match. Stripped from the text *and* from the phrases
/// before comparing, so `ABSENCE_PHRASES` can stay written in natural English.
const HEDGE_WORDS: &[&str] = &[
    "actively",
    "currently",
    "still",
    "truly",
    "genuinely",
    "really",
    "properly",
    "widely",
];

/// Lowercase `text` and drop the hedges above, so "no currently maintained tool"
/// collapses to the "no maintained tool" the phrase list is written against.
fn strip_hedges(text: &str) -> String {
    let mut out = text.to_lowercase();
    for hedge in HEDGE_WORDS {
        out = out.replace(&format!("{hedge} "), "");
    }
    out
}

/// True if `text` asserts that something does not exist.
fn contains_absence_phrase(text: &str) -> bool {
    let lower = text.to_lowercase();
    let hedgeless = strip_hedges(&lower);
    ABSENCE_PHRASES
        .iter()
        .any(|p| lower.contains(p) || hedgeless.contains(&strip_hedges(p)))
}

/// Phrases claiming nothing was found. Fine when matches are weak, but misleading
/// when a genuinely close match is present, so they are guarded against below.
const NO_MATCH_PHRASES: &[&str] = &[
    "no direct match",
    "no close match",
    "no matching",
    "no matches found",
    "no match found",
    "no relevant match",
    "no clear match",
    "no exact match",
    "nothing closely related",
    "no direct prior art",
    "couldn't find any",
    "could not find any",
];

/// True if `text` claims nothing was found.
fn claims_no_match(text: &str) -> bool {
    let lower = text.to_lowercase();
    NO_MATCH_PHRASES.iter().any(|p| lower.contains(p))
}

/// A safe, scoped headline derived purely from the data — never asserts absence.
fn data_headline(level: Saturation, matches: &[Match]) -> String {
    let close = matches.iter().filter(|m| m.similarity >= 0.55).count();
    match level {
        Saturation::Saturated => {
            format!("Saturated — {close} closely-related tools turned up in the sources checked.")
        }
        Saturation::Crowded => format!(
            "Crowded — {close} closely-related tool{} turned up in the sources checked.",
            if close == 1 { "" } else { "s" }
        ),
        Saturation::Open => {
            if close == 0 {
                "Nothing close turned up in the sources checked — keep looking before committing."
                    .to_string()
            } else {
                format!(
                    "{close} closely-related tool{} turned up, but the space still looks open in the sources checked. Worth a look before committing.",
                    if close == 1 { "" } else { "s" }
                )
            }
        }
    }
}

/// Replace any headline that asserts non-existence with a safe scoped one. This
/// is the code-level guarantee behind the integrity rule; the prompt asks the
/// model to comply, but we never *rely* on it.
fn guard_headline(headline: String, level: Saturation, matches: &[Match]) -> String {
    if contains_absence_phrase(&headline) {
        data_headline(level, matches)
    } else {
        headline
    }
}

/// Floor the model's level against the similarity data so it can never hand out
/// a green-light "Open" when the embeddings clearly show close prior art.
fn floor_level(model_level: Saturation, matches: &[Match]) -> Saturation {
    let strong = matches.iter().filter(|m| m.similarity >= 0.60).count();
    let close = matches.iter().filter(|m| m.similarity >= 0.55).count();
    // A single near-identical match (>= 0.70) already means the space isn't open.
    let very_strong = matches.iter().filter(|m| m.similarity >= 0.70).count();
    let data_level = if strong >= 5 {
        Saturation::Saturated
    } else if close >= 2 || very_strong >= 1 {
        Saturation::Crowded
    } else {
        Saturation::Open
    };
    model_level.max(data_level)
}

/// True if `word` appears in `text` as a whole word (not as part of another word).
fn is_whole_word(text: &str, word: &str) -> bool {
    if word.is_empty() {
        return false;
    }
    let mut remaining = text;
    while let Some(pos) = remaining.find(word) {
        let before_ok = remaining[..pos]
            .chars()
            .next_back()
            .is_none_or(|c| !c.is_alphanumeric());
        let after_ok = remaining[pos + word.len()..]
            .chars()
            .next()
            .is_none_or(|c| !c.is_alphanumeric());
        if before_ok && after_ok {
            return true;
        }
        remaining = &remaining[pos + word.len()..];
        if remaining.is_empty() {
            break;
        }
    }
    false
}

/// Extract JSON from a model response that may be wrapped in markdown fences.
fn extract_json(raw: &str) -> &str {
    let mut trimmed = raw.trim();
    // Reasoning models in the DeepSeek-R1 family emit their trace inline as a
    // `<think>...</think>` preamble instead of in a separate field. The trace
    // often drafts a fenced JSON block of its own, so it has to be dropped
    // before the fence scan below -- otherwise we parse the draft rather than
    // the answer. Slice past the last closing tag rather than matching a pair:
    // some chat templates open the block for the model, so the response can
    // carry `</think>` with no opener. An unterminated trace is left alone and
    // fails to parse, which is the honest outcome.
    if let Some(end) = trimmed.rfind("</think>") {
        trimmed = trimmed[end + "</think>".len()..].trim();
    }
    if let Some(start) = trimmed.find("```") {
        let after_fence = &trimmed[start + 3..];
        let content = after_fence
            .strip_prefix("json")
            .unwrap_or(after_fence)
            .trim_start();
        if let Some(end) = content.find("```") {
            return content[..end].trim();
        }
    }
    trimmed
}

/// Parse the model's JSON response into the verdict fields we need, then apply
/// the two integrity guards: floor the level against the similarity data, and
/// replace any headline that asserts non-existence.
fn parse_verdict(
    raw: &str,
    matches: &[Match],
    sources_checked: Vec<Source>,
    sources_failed: Vec<Source>,
) -> crate::Result<Verdict> {
    let json_str = extract_json(raw);

    let v: serde_json::Value =
        serde_json::from_str(json_str).map_err(|e| crate::Error::Parse(e.to_string()))?;

    let model_level = match v["level"].as_str() {
        Some("Open") => Saturation::Open,
        Some("Crowded") => Saturation::Crowded,
        Some("Saturated") => Saturation::Saturated,
        other => return Err(crate::Error::Parse(format!("invalid level: {:?}", other))),
    };

    let raw_headline = v["headline"]
        .as_str()
        .ok_or_else(|| crate::Error::Parse("missing 'headline'".into()))?
        .to_string();

    // Gaps render verbatim, so they get the same absence-claim guard as the
    // headline: a gap that asserts non-existence is dropped rather than shown.
    // Also drop any gap that names a top-10 match — if the model mentions a
    // known tool in a gap it is confirming it exists, not identifying open space.
    let top_names: Vec<String> = matches
        .iter()
        .take(10)
        .map(|m| m.name.to_lowercase())
        .collect();
    let gaps: Vec<String> = match v["gaps"].as_array() {
        Some(arr) => arr
            .iter()
            .filter_map(|g| g.as_str().map(String::from))
            .filter(|g| !contains_absence_phrase(g))
            .filter(|g| {
                let lower = g.to_lowercase();
                !top_names.iter().any(|name| is_whole_word(&lower, name))
            })
            .collect(),
        None => vec![],
    };

    // Floor the level against the data. If that raises it, the model misjudged
    // the space, so we don't trust its headline either — derive a safe one.
    let level = floor_level(model_level, matches);
    let headline = if level != model_level {
        data_headline(level, matches)
    } else {
        raw_headline
    };
    let headline = guard_headline(headline, level, matches);

    // A close match (>= 0.55) is real prior art, so a "found nothing" headline
    // would be misleading even when the level stays Open. Replace it with the
    // data-derived headline, which names the close matches.
    let close = matches.iter().filter(|m| m.similarity >= 0.55).count();
    let headline = if close >= 1 && claims_no_match(&headline) {
        data_headline(level, matches)
    } else {
        headline
    };

    // When level is Open and nothing close turned up, always use the data
    // headline. The LLM may emit confident-sounding copy that reads as a green
    // light even when the matches are too weak to trust. The data headline is
    // the honest signal here.
    let headline = if level == Saturation::Open && close == 0 {
        data_headline(level, matches)
    } else {
        headline
    };

    Ok(Verdict {
        level,
        headline,
        gaps,
        sources_checked,
        sources_failed,
        caveat: CAVEAT.to_string(),
    })
}

/// Build a verdict from the similarity data alone, without calling a model.
///
/// This is the `--fast` path (and any caller that deliberately skips Ollama).
/// The saturation level is derived purely by `floor_level`-ing against the
/// embeddings and the headline is the same safe, scoped, data-only sentence the
/// flooring guard produces — so a no-LLM run still gives an honest signal,
/// never flashes a misleading green "Open" over a clearly-populated space, and
/// still carries the fixed integrity [`CAVEAT`]. Gaps require a model, so they
/// are empty here.
pub fn from_data(
    matches: &[Match],
    sources_checked: Vec<Source>,
    sources_failed: Vec<Source>,
) -> Verdict {
    let level = floor_level(Saturation::Open, matches);
    Verdict {
        headline: data_headline(level, matches),
        level,
        gaps: vec![],
        sources_checked,
        sources_failed,
        caveat: CAVEAT.to_string(),
    }
}

/// Produce a [`Verdict`] from ranked matches via an [`Llm`] backend.
pub async fn assess(
    llm: &dyn Llm,
    query: &Query,
    matches: &[Match],
    sources_checked: Vec<Source>,
    sources_failed: Vec<Source>,
) -> crate::Result<Verdict> {
    let prompt = build_prompt(query, matches, &sources_checked);
    let raw = match llm.generate(&prompt).await {
        Ok(r) => r,
        Err(crate::Error::Http(_) | crate::Error::LlmUnreachable(_)) => {
            tokio::time::sleep(std::time::Duration::from_millis(800)).await;
            llm.generate(&prompt).await?
        }
        Err(e) => return Err(e),
    };
    parse_verdict(&raw, matches, sources_checked, sources_failed)
}
