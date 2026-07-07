//! `patent` binary — thin CLI/TUI shell over the `patent` library.

mod cli;
mod config;
mod pipeline;
mod tui;

use clap::{CommandFactory, Parser};
use cli::Cli;
use std::io::IsTerminal;

fn validate_idea(idea: &str) -> anyhow::Result<()> {
    let trimmed = idea.trim();
    if trimmed.is_empty() {
        anyhow::bail!("Please provide a dev-tool idea to search for.");
    }

    // Unicode-aware: a "word" is a whitespace-delimited token with at least
    // three alphanumeric characters in any script (not just ASCII).
    let meaningful: Vec<String> = trimmed
        .split_whitespace()
        .filter(|w| w.chars().filter(|c| c.is_alphanumeric()).count() >= 3)
        .map(|w| w.to_lowercase())
        .collect();

    // Scripts that don't separate words with spaces (e.g. Chinese, Japanese)
    // can't be tokenized into words this way. Accept them based on a count of
    // non-ASCII alphanumeric characters — gating on non-ASCII so this doesn't
    // become an escape hatch that lets short ASCII gibberish skip the checks
    // below.
    let space_delimited = trimmed.split_whitespace().count() >= 3;
    let cjk_like = trimmed
        .chars()
        .filter(|c| c.is_alphanumeric() && !c.is_ascii())
        .count();
    if !space_delimited && cjk_like >= 2 {
        return Ok(());
    }

    if meaningful.len() < 3 {
        anyhow::bail!(
            "Too vague — describe a specific software tool or feature, e.g.\n  \
             patent \"CLI tool that kills a process on a given port\""
        );
    }

    let unique: std::collections::HashSet<&str> = meaningful.iter().map(|w| w.as_str()).collect();
    if unique.len() < 3 {
        anyhow::bail!(
            "Too repetitive — describe what the tool does, e.g.\n  \
             patent \"CLI tool that kills a process on a given port\""
        );
    }

    let non_stopword_count = meaningful
        .iter()
        .filter(|w| !STOPWORDS.contains(&w.as_str()))
        .count();
    if non_stopword_count < 2 {
        anyhow::bail!(
            "Too vague — describe a specific software tool or feature, e.g.\n  \
             patent \"CLI tool that kills a process on a given port\""
        );
    }

    Ok(())
}

const STOPWORDS: &[&str] = &[
    "the", "and", "for", "are", "but", "not", "you", "all", "can", "had", "her", "was", "one",
    "our", "out", "has", "its", "let", "may", "who", "did", "get", "got", "how", "his", "him",
    "she", "also", "been", "call", "each", "from", "have", "into", "just", "like", "long", "make",
    "many", "more", "most", "much", "must", "name", "only", "over", "some", "such", "than", "that",
    "them", "then", "they", "this", "very", "when", "what", "with", "will", "your", "which",
    "about", "after", "being", "could", "every", "first", "found", "great", "where", "these",
    "their", "there", "those", "would", "other", "should", "before", "between", "best", "near",
    "here", "well", "does", "were",
];

fn strip_punctuation(word: &str) -> String {
    word.chars()
        .filter(|c| c.is_alphanumeric() || *c == '-')
        .collect()
}

fn build_query(idea: &str) -> patent::Query {
    let keywords: Vec<String> = idea
        .split_whitespace()
        .map(|w| strip_punctuation(&w.to_lowercase()))
        .filter(|w| w.len() > 2 && !STOPWORDS.contains(&w.as_str()))
        .collect();
    patent::Query {
        idea: idea.to_string(),
        keywords,
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Cli::parse();

    if let Some(shell) = args.completions {
        clap_complete::generate(shell, &mut Cli::command(), "patent", &mut std::io::stdout());
        return Ok(());
    }

    // Load config and resolve backend settings before the no-idea check so the
    // interactive TUI gets the configured LLM backend when no idea is on the CLI.
    let cfg = config::load()?;

    // Resolve: CLI flag / env var (via clap) > config file > built-in default.
    // OPENAI_API_KEY is a well-known fallback for api_key only (checked last).
    let api_base = args.api_base.or(cfg.api_base);
    let api_key_explicit = args.api_key.is_some() || cfg.api_key.is_some();
    let api_key = args
        .api_key
        .or(cfg.api_key)
        .or_else(|| std::env::var("OPENAI_API_KEY").ok());
    let model = args.model.or(cfg.model);

    if !args.fast && api_base.is_some() && model.is_none() {
        anyhow::bail!(
            "api_base is set but no model was specified. \
             Pass --model <NAME>, set PATENT_MODEL, or add `model = \"...\"` to \
             your patent config file (its location is shown in the README)."
        );
    }
    if api_key_explicit && api_base.is_none() {
        eprintln!("warning: api_key has no effect without api_base; using local Ollama.");
    }
    if args.fast && api_base.is_some() {
        eprintln!("warning: --fast skips the LLM, so api_base has no effect.");
    }
    if let Some(base) = &api_base {
        if !args.fast {
            eprintln!(
                "note: --api-base sends your search query to {base} to generate the verdict; \
                 the default local Ollama backend and --fast keep it on your machine."
            );
        }
    }

    let idea = match args.idea {
        Some(i) => i,
        None => {
            if args.json {
                anyhow::bail!("--json requires an idea argument.");
            }
            if !std::io::stdout().is_terminal() {
                anyhow::bail!("No idea provided. Usage: patent \"your dev-tool idea here\"");
            }
            return tui::run_interactive(tui::TuiCfg {
                api_base,
                api_key,
                model,
                fast: args.fast,
                keyword_only: args.keyword_only,
            })
            .await;
        }
    };
    validate_idea(&idea)?;

    let query = build_query(&idea);
    eprintln!("Searching for prior art: \"{}\"", idea);
    eprintln!("   keywords: {}", query.keywords.join(", "));

    if !args.keyword_only && !patent::rank::model_is_cached() {
        eprintln!(
            "patent: downloading the embedding model for local semantic search (~80 MB, one-time)..."
        );
    }

    let t_start = std::time::Instant::now();
    let result = pipeline::run(
        &idea,
        &pipeline::PipelineCfg {
            api_base: api_base.clone(),
            api_key: api_key.clone(),
            model: model.clone(),
            fast: args.fast,
            keyword_only: args.keyword_only,
            limit: args.limit as usize,
        },
        |msg| eprintln!("{msg}"),
    )
    .await?;
    let verdict = result.verdict;
    let ranked = result.matches;

    eprintln!("total: {:.1}s", t_start.elapsed().as_secs_f64());

    // ── Phase 4: output ─────────────────────────────────────────────────
    // The TUI needs a real terminal; when stdout is piped or redirected, fall
    // back to JSON rather than panicking on terminal initialization.
    let exit_code = verdict.level.exit_code();

    let want_json = args.json || !std::io::stdout().is_terminal();
    if want_json {
        if !args.json {
            eprintln!(
                "note: stdout is not a terminal — emitting JSON (pass --json to silence this)."
            );
        }
        #[derive(serde::Serialize)]
        struct Output {
            schema_version: u32,
            query: String,
            verdict: patent::Verdict,
            matches: Vec<patent::Match>,
        }
        let output = Output {
            schema_version: 1,
            query: idea.clone(),
            verdict,
            matches: ranked,
        };
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        tui::run_with_results(
            &idea,
            verdict,
            ranked,
            tui::TuiCfg {
                api_base,
                api_key,
                model,
                fast: args.fast,
                keyword_only: args.keyword_only,
            },
        )
        .await?;
    }

    std::process::exit(exit_code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_idea_accepts_a_normal_idea() {
        assert!(validate_idea("a cli tool to kill a process on a given port").is_ok());
    }

    #[test]
    fn validate_idea_rejects_empty_and_vague() {
        assert!(validate_idea("").is_err());
        assert!(validate_idea("   ").is_err());
        assert!(validate_idea("ab cd").is_err());
        // Regression: short ASCII gibberish must NOT slip through the CJK
        // fallback (which only applies to non-ASCII, space-free scripts).
        assert!(validate_idea("foobar").is_err());
        assert!(validate_idea("123456").is_err());
        assert!(validate_idea("hello world").is_err());
    }

    #[test]
    fn validate_idea_rejects_fewer_than_two_non_stopwords() {
        // Zero non-stopwords — build_query produces empty keywords.
        assert!(validate_idea("the and for are but not").is_err());
        assert!(validate_idea("those would other great found first").is_err());
        // One non-stopword — "tool" alone generates noise results for any query.
        assert!(validate_idea("what does this tool have with them").is_err());
        assert!(validate_idea("the and for tool are but not").is_err());
        // Two non-stopwords — acceptable minimum.
        assert!(validate_idea("the and for tool linting what").is_ok());
    }

    #[test]
    fn validate_idea_accepts_non_ascii_scripts() {
        // Cyrillic (space-delimited) and CJK (no spaces) must both be accepted.
        assert!(validate_idea("инструмент для управления процессами на порту").is_ok());
        assert!(validate_idea("端口杀手命令行工具").is_ok());
    }
}
