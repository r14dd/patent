//! Shared search → rank → verdict pipeline used by both the CLI and TUI paths.

use patent::model::{Saturation, Verdict};

/// Matches below this similarity are noise, not signal — the LLM verdict is
/// skipped entirely to avoid hallucinating a judgement from irrelevant results.
const MIN_RELEVANCE: f32 = 0.35;

pub struct PipelineCfg {
    pub api_base: Option<String>,
    pub api_key: Option<String>,
    pub model: Option<String>,
    pub fast: bool,
    pub keyword_only: bool,
    pub limit: usize,
    pub sources_include: Option<std::collections::HashSet<patent::Source>>,
    pub sources_exclude: std::collections::HashSet<patent::Source>,
}

pub struct PipelineResult {
    pub verdict: Verdict,
    pub matches: Vec<patent::Match>,
}

pub async fn run(
    idea: &str,
    cfg: &PipelineCfg,
    log: impl Fn(&str) + Send + Sync,
) -> anyhow::Result<PipelineResult> {
    let query = crate::build_query(idea);

    // ── Phase 1: search sources (+ load embedding model if needed) ──────
    let t_start = std::time::Instant::now();
    let eval_limit = cfg.limit.max(patent::rank::DEFAULT_LIMIT);

    let has_filter = cfg.sources_include.is_some() || !cfg.sources_exclude.is_empty();

    let (search_result, ranked) = if cfg.keyword_only {
        let search_result = if has_filter {
            patent::sources::search_filtered(
                &query,
                cfg.sources_include.as_ref(),
                &cfg.sources_exclude,
            )
            .await
        } else {
            patent::sources::search_all(&query).await
        };
        let patent::sources::SearchOutcome {
            matches: raw_matches,
            reached,
            failed,
        } = search_result?;
        log(&format!(
            "   {} matches from {} sources in {:.1}s: {}",
            raw_matches.len(),
            reached.len(),
            t_start.elapsed().as_secs_f64(),
            reached
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ));

        let t_rank = std::time::Instant::now();
        let ranked = patent::rank::rank_by_keywords(&query, raw_matches, eval_limit);
        log(&format!(
            "Keyword-ranked to top {} in {:.1}s",
            ranked.len(),
            t_rank.elapsed().as_secs_f64(),
        ));
        ((reached, failed), ranked)
    } else {
        let idea_for_embed = query.idea.clone();
        let (search_result, ranker_result) = tokio::join!(
            async {
                if has_filter {
                    patent::sources::search_filtered(
                        &query,
                        cfg.sources_include.as_ref(),
                        &cfg.sources_exclude,
                    )
                    .await
                } else {
                    patent::sources::search_all(&query).await
                }
            },
            tokio::task::spawn_blocking(move || {
                let mut ranker = patent::rank::Ranker::new()?;
                let query_emb = ranker.embed_query(&idea_for_embed)?;
                Ok::<_, patent::Error>((ranker, query_emb))
            })
        );

        let patent::sources::SearchOutcome {
            matches: raw_matches,
            reached,
            failed,
        } = search_result?;
        let (mut ranker, query_emb) =
            ranker_result.map_err(|e| anyhow::anyhow!("embedding task panicked: {e}"))??;

        log(&format!(
            "   {} matches from {} sources in {:.1}s: {}",
            raw_matches.len(),
            reached.len(),
            t_start.elapsed().as_secs_f64(),
            reached
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ));

        // ── Phase 2: rank (embed descriptions + cosine sort) ────────────
        // Rank with max(limit, DEFAULT_LIMIT) so floor_level and the verdict
        // prompt see enough data to make an accurate call even when --limit
        // is small.  The result is truncated to `limit` after the verdict.
        let t_rank = std::time::Instant::now();
        let ranked = tokio::task::spawn_blocking(move || {
            ranker.rank_with(&query_emb, raw_matches, eval_limit)
        })
        .await
        .map_err(|e| anyhow::anyhow!("ranking task panicked: {e}"))??;
        log(&format!(
            "Ranked to top {} in {:.1}s",
            ranked.len(),
            t_rank.elapsed().as_secs_f64(),
        ));
        ((reached, failed), ranked)
    };
    let (reached, failed) = search_result;

    // ── Phase 3: relevance gate + verdict ───────────────────────────────
    let fallback = |headline: &str| Verdict {
        level: Saturation::Open,
        headline: headline.to_string(),
        gaps: vec![],
        sources_checked: reached.clone(),
        sources_failed: failed.clone(),
        caveat: patent::verdict::CAVEAT.to_string(),
    };

    let best_sim = ranked.first().map_or(0.0, |m| m.similarity);
    let verdict = if cfg.fast {
        log("--fast: skipping the LLM (verdict from similarity data only)");
        patent::verdict::from_data(&ranked, reached, failed)
    } else if best_sim < MIN_RELEVANCE {
        log(&format!(
            "warning: best similarity {:.2} < {:.2} — skipping verdict",
            best_sim, MIN_RELEVANCE,
        ));
        fallback(
            "Nothing relevant turned up in the sources checked. \
             The query may not describe a recognized software tool — \
             try rephrasing with specific technical terms.",
        )
    } else {
        let model_name = cfg
            .model
            .clone()
            .unwrap_or_else(|| patent::ollama::DEFAULT_MODEL.to_string());
        let llm: Box<dyn patent::Llm> = match &cfg.api_base {
            Some(base) => Box::new(patent::openai::OpenAi::new(
                base.clone(),
                model_name.clone(),
                cfg.api_key.clone(),
            )?),
            None => Box::new(patent::ollama::Ollama::new(
                patent::ollama::DEFAULT_ENDPOINT,
                model_name.clone(),
            )?),
        };

        let t_verdict = std::time::Instant::now();
        log(&format!(
            "Generating verdict via {} ({})...",
            llm.label(),
            model_name
        ));
        match patent::verdict::assess(&*llm, &query, &ranked, reached.clone(), failed.clone()).await
        {
            Ok(v) => {
                log(&format!(
                    "   verdict in {:.1}s",
                    t_verdict.elapsed().as_secs_f64()
                ));
                v
            }
            Err(e) => {
                log(&format!("warning: {e}"));
                log("   showing results without an AI verdict.");
                fallback("Verdict unavailable — results are ranked by semantic similarity only.")
            }
        }
    };

    let matches = ranked.into_iter().take(cfg.limit).collect();

    Ok(PipelineResult { verdict, matches })
}
