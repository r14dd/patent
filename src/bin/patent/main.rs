//! `patent` binary — thin CLI/TUI shell over the `patent` library.

mod cli;
mod config;
mod tui;

use clap::{CommandFactory, Parser};
use cli::Cli;
use patent::pipeline::{self, VerdictNote};
use std::io::IsTerminal;

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

    let llm_cfg = pipeline::LlmConfig {
        api_base: api_base.clone(),
        api_key: api_key.clone(),
        model: model.clone(),
        fast: args.fast,
    };

    let idea = match args.idea {
        Some(i) => i,
        None => {
            if args.json {
                anyhow::bail!("--json requires an idea argument.");
            }
            if !std::io::stdout().is_terminal() {
                anyhow::bail!("No idea provided. Usage: patent \"your dev-tool idea here\"");
            }
            return tui::run_interactive(tui::TuiCfg { llm: llm_cfg }).await;
        }
    };
    pipeline::validate_idea(&idea).map_err(|msg| anyhow::anyhow!("{msg}"))?;

    let query = pipeline::build_query(&idea);
    eprintln!("Searching for prior art: \"{}\"", idea);
    eprintln!("   keywords: {}", query.keywords.join(", "));

    if !patent::rank::model_is_cached() {
        eprintln!(
            "patent: downloading the embedding model for local semantic search (~80 MB, one-time)..."
        );
    }

    let t_start = std::time::Instant::now();
    let limit = args.limit as usize;
    let outcome = pipeline::run(&idea, &llm_cfg, Some(limit)).await?;

    eprintln!(
        "   {} matches from {} sources in {:.1}s: {}",
        outcome.matches.len(),
        outcome.verdict.sources_checked.len(),
        outcome.search_secs,
        outcome
            .verdict
            .sources_checked
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>()
            .join(", ")
    );
    eprintln!(
        "Ranked to top {} in {:.1}s",
        outcome.matches.len(),
        outcome.rank_secs,
    );

    match &outcome.verdict_note {
        VerdictNote::Fast => {
            eprintln!("--fast: skipping the LLM (verdict from similarity data only)");
        }
        VerdictNote::BelowRelevance { best_sim } => {
            eprintln!("warning: best similarity {best_sim:.2} < 0.35 — skipping verdict",);
        }
        VerdictNote::Llm { label, model, secs } => {
            eprintln!("Generating verdict via {label} ({model})...");
            eprintln!("   verdict in {secs:.1}s");
        }
        VerdictNote::LlmFailed { error } => {
            eprintln!("warning: {error}");
            eprintln!("   showing results without an AI verdict.");
        }
    }

    let ranked: Vec<_> = outcome.matches.into_iter().take(limit).collect();

    eprintln!("total: {:.1}s", t_start.elapsed().as_secs_f64());

    // ── Output ──────────────────────────────────────────────────────────
    let exit_code = outcome.verdict.level.exit_code();

    let want_json = args.json || !std::io::stdout().is_terminal();
    if want_json {
        if !args.json {
            eprintln!(
                "note: stdout is not a terminal — emitting JSON (pass --json to silence this)."
            );
        }
        let output = serde_json::json!({
            "query": idea,
            "verdict": outcome.verdict,
            "matches": ranked,
        });
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        tui::run_with_results(&idea, outcome.verdict, ranked, tui::TuiCfg { llm: llm_cfg }).await?;
    }

    std::process::exit(exit_code)
}

#[cfg(test)]
mod tests {
    use patent::pipeline;

    #[test]
    fn validate_idea_accepts_a_normal_idea() {
        assert!(pipeline::validate_idea("a cli tool to kill a process on a given port").is_ok());
    }

    #[test]
    fn validate_idea_rejects_empty_and_vague() {
        assert!(pipeline::validate_idea("").is_err());
        assert!(pipeline::validate_idea("   ").is_err());
        assert!(pipeline::validate_idea("ab cd").is_err());
        assert!(pipeline::validate_idea("foobar").is_err());
        assert!(pipeline::validate_idea("123456").is_err());
        assert!(pipeline::validate_idea("hello world").is_err());
    }

    #[test]
    fn validate_idea_rejects_fewer_than_two_non_stopwords() {
        assert!(pipeline::validate_idea("the and for are but not").is_err());
        assert!(pipeline::validate_idea("those would other great found first").is_err());
        assert!(pipeline::validate_idea("what does this tool have with them").is_err());
        assert!(pipeline::validate_idea("the and for tool are but not").is_err());
        assert!(pipeline::validate_idea("the and for tool linting what").is_ok());
    }

    #[test]
    fn validate_idea_accepts_non_ascii_scripts() {
        assert!(pipeline::validate_idea("инструмент для управления процессами на порту").is_ok());
        assert!(pipeline::validate_idea("端口杀手命令行工具").is_ok());
    }
}
