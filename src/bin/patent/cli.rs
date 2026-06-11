//! Command-line argument parsing.

use clap::Parser;

/// A prior-art search for your code ideas.
#[derive(Debug, Parser)]
#[command(name = "patent", version, about)]
pub struct Cli {
    /// The dev-tool idea to search for, e.g.
    /// "interactive cli to kill whatever's on a port".
    #[arg(required_unless_present = "completions")]
    pub idea: Option<String>,

    /// Max number of matches to keep after ranking (must be at least 1).
    #[arg(long, default_value_t = patent::rank::DEFAULT_LIMIT as u32, value_parser = clap::value_parser!(u32).range(1..))]
    pub limit: u32,

    /// LLM model for the verdict. Defaults to qwen2.5 for Ollama; required with --api-base.
    /// Can also be set via PATENT_MODEL or config file.
    #[arg(long, env = "PATENT_MODEL")]
    pub model: Option<String>,

    /// Use an OpenAI-compatible API instead of local Ollama. Base URL ending in
    /// /v1, e.g. https://api.openai.com/v1 or http://localhost:1234/v1.
    /// Can also be set via PATENT_API_BASE or config file.
    #[arg(long, value_name = "URL", env = "PATENT_API_BASE")]
    pub api_base: Option<String>,

    /// API key for --api-base. Falls back to PATENT_API_KEY, config file, then
    /// OPENAI_API_KEY. Omit for servers without auth.
    #[arg(long, value_name = "KEY", env = "PATENT_API_KEY")]
    pub api_key: Option<String>,

    /// Skip the LLM verdict for an instant, search-only result.
    #[arg(long)]
    pub fast: bool,

    /// Print structured JSON instead of launching the TUI.
    #[arg(long)]
    pub json: bool,

    /// Generate shell completions and exit.
    #[arg(long, value_name = "SHELL")]
    pub completions: Option<clap_complete::Shell>,
}
