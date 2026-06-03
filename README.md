# patent

**A prior-art search for your code ideas.** Stop building what already exists —
find the state of the art in seconds, without leaving the terminal.

[![downloads](https://img.shields.io/crates/d/patent.svg)](https://crates.io/crates/patent)
[![docs.rs](https://docs.rs/patent/badge.svg)](https://docs.rs/patent)
[![CI](https://github.com/r14dd/patent/actions/workflows/ci.yml/badge.svg)](https://github.com/r14dd/patent/actions/workflows/ci.yml)
[![GitHub stars](https://img.shields.io/github/stars/r14dd/patent?style=flat&logo=github)](https://github.com/r14dd/patent/stargazers)
[![license: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)
[![Built With Ratatui](https://ratatui.rs/built-with-ratatui/badge.svg)](https://ratatui.rs/)

<p align="center">
  <img src="https://raw.githubusercontent.com/r14dd/patent/main/demo.gif" alt="patent in action" width="720">
</p>

Give `patent` a plain-English dev-tool idea and it searches 11 open-source
registries — crates.io, npm, PyPI, GitHub, Docker Hub, and more — ranks the
matches with local semantic search, and writes an honest verdict on whether the
space is **Open**, **Crowded**, or **Saturated**. Everything runs on your
machine.

```bash
cargo install patent
```

```bash
patent "interactive cli to kill whatever's on a port"
```

> Like a patent search, but for code. A patent search *finds prior art* — it
> never certifies absence. Neither does this.

## Why

- **Prevent wasted effort** — find out something already exists *before* you
  spend a month rebuilding it.
- **Find inspiration & benchmarks** — see the closest prior art, ranked, so you
  know exactly what you'd have to beat.
- **Competitive analysis** — survey an entire ecosystem in one command instead
  of tab-hopping across a dozen registries.

## Quick demo

```bash
# A crowded space — lots of prior art, ranked by relevance
patent "distributed key-value store in rust"

# Pipe structured output into your own tooling
patent "react component for infinite scroll" --json | jq .

# Skip the AI verdict for an instant, search-only result
patent "kubernetes log viewer" --fast
```

## Features

- **Search the entire developer ecosystem at once** — 11 registries (crates.io,
  npm, PyPI, GitHub, Go, Maven, NuGet, RubyGems, Docker Hub, VS Code
  Marketplace, Hacker News) in a single command.
- **It searches the right places automatically** — mention "rust" and it hits
  crates.io; mention "docker" and it hits Docker Hub. GitHub and Hacker News
  (both language-agnostic) are always searched; with no language signal it falls
  back to a broad sweep across the largest registries.
- **Relevance you can trust** — local embeddings (AllMiniLM-L6-V2 via
  [fastembed](https://crates.io/crates/fastembed)) rank every match by cosine
  similarity to your idea, so the closest prior art floats to the top.
- **An honest verdict, not hype** — a local [Ollama](https://ollama.com) model
  (or any OpenAI-compatible API via `--api-base`) classifies the space as Open /
  Crowded / Saturated and names the gaps you could fill. It can prove something
  *exists*; it never claims something *doesn't*.
- **Instant mode** — `--fast` skips the LLM and gives you the ranked list
  immediately, with a saturation level derived straight from the similarity
  data.
- **See it, don't parse it** — an interactive TUI with scrollable, filterable,
  sortable matches, a detail popup for any result, mouse support, and one-key
  URL opening. Need machine output? `--json`.
- **Local-first & private** — embeddings always run on your machine, and the
  verdict uses a local LLM by default; point `--api-base` at a cloud API only if
  you want to.
- **Never fails loud** — LLM down, model not pulled, or a cloud API rejecting
  the request? Results still render, ranked by similarity, without an AI verdict.
  A source fails? It's skipped (and shown as "not reached"), never fatal.

## Install

### From crates.io

```bash
cargo install patent
```

### From source

```bash
git clone https://github.com/r14dd/patent.git
cd patent
cargo install --path .
```

### Prerequisites

**Rust** (stable 1.80+) via [rustup](https://rustup.rs).

**Ollama** (optional, recommended) — powers the AI verdict. Skip it and use
`--fast`, or just let `patent` fall back to a similarity-only verdict:

```bash
# macOS
brew install ollama

# Linux
curl -fsSL https://ollama.com/install.sh | sh

# Windows (or download the installer from https://ollama.com/download)
winget install Ollama.Ollama

# Then (any platform):
ollama pull qwen2.5
ollama serve
```

**Cloud or OpenAI-compatible API** (alternative to Ollama) — point `patent` at
any server that speaks the OpenAI chat API with `--api-base`, plus `--api-key`
(or the `OPENAI_API_KEY` env var):

```bash
# OpenAI
patent "vector database" --api-base https://api.openai.com/v1 --model gpt-4o-mini --api-key sk-...

# OpenRouter
patent "vector database" --api-base https://openrouter.ai/api/v1 --model openai/gpt-4o-mini --api-key sk-or-...

# Local LM Studio / llama.cpp / vLLM (usually no key needed)
patent "vector database" --api-base http://localhost:1234/v1 --model your-model
```

**GitHub token** (optional) — the unauthenticated GitHub search API is limited
to 10 requests/minute. Set a token to raise that to 30 requests/minute (3×):

```bash
# macOS / Linux
export GITHUB_TOKEN=ghp_your_token_here
```

```powershell
# Windows (PowerShell)
$env:GITHUB_TOKEN = "ghp_your_token_here"
```

**First run** — `patent` downloads a small (~80 MB) embedding model the first
time it ranks results. You'll see a one-time `downloading the embedding model…`
notice. It's cached under your OS cache directory (e.g. `~/Library/Caches/patent`
on macOS, `~/.cache/patent` on Linux, `%LOCALAPPDATA%\patent` on Windows), so
it's a one-time download shared across every directory you run from.

## Usage

```bash
# Basic search — opens the interactive TUI
patent "CLI tool that kills whatever's on a port"

# Instant, search-only result — no model warm-up, no inference wait
patent "CLI tool that kills whatever's on a port" --fast

# Structured JSON output for scripting
patent "react component for infinite scroll" --json | jq .

# Use a smaller/faster Ollama model
patent "kubernetes log viewer" --model qwen2.5:3b

# Use a cloud LLM instead of local Ollama
patent "kubernetes log viewer" --api-base https://api.openai.com/v1 --model gpt-4o-mini

# Keep more matches after ranking
patent "async runtime for rust" --limit 100
```

### Options

| Flag | Description | Default |
|---|---|---|
| `--fast` | Skip the LLM verdict for an instant, search-only result | — |
| `--json` | Print JSON to stdout instead of the TUI | — |
| `--model <MODEL>` | LLM model for the verdict | `qwen2.5` (Ollama) |
| `--api-base <URL>` | Use an OpenAI-compatible API (base URL ending in `/v1`) instead of Ollama | — |
| `--api-key <KEY>` | API key for `--api-base` (or set `OPENAI_API_KEY`) | — |
| `--limit <N>` | Max matches to keep after ranking | `50` |
| `--completions <SHELL>` | Generate shell completions and exit | — |

### Shell completions

```bash
# Bash
patent --completions bash >> ~/.bashrc

# Zsh
patent --completions zsh >> ~/.zshrc

# Fish
patent --completions fish > ~/.config/fish/completions/patent.fish

# PowerShell
patent --completions powershell >> $PROFILE
```

## TUI keybindings

| Key | Action |
|---|---|
| `↑` / `k` | Scroll up |
| `↓` / `j` | Scroll down |
| `g` / `Home` | Jump to top |
| `G` / `End` | Jump to bottom |
| `/` | Filter matches |
| `s` | Cycle sort (similarity / popularity / name) |
| `m` | Show more / show less |
| `Enter` | Show match details (full description, popularity, URL) |
| `o` | Open selected match in browser |
| `?` | Help overlay |
| `q` | Quit |

The mouse works too — scroll with the wheel, click a row to select it. Press
`?` inside the TUI for the full keybinding reference.

## How it works

```
idea ──► parse keywords
              │
              ├──► fan out to sources (concurrent, with retry)
              │         │
              │         ▼
              │    dedup matches
              │         │
              ▼         ▼
         load model ──► rank by cosine similarity
                              │
                              ▼
                        verdict via LLM
                              │
                              ▼
                        TUI or JSON
```

1. **Parse** — extracts keywords, selects relevant sources based on the idea
2. **Search** — fans out to selected sources concurrently; a failing source is
   skipped and retried once, never fatal
3. **Rank** — embeds the idea and each match description with AllMiniLM-L6-V2,
   sorts by cosine similarity, keeps the top N
4. **Verdict** — sends the ranked matches to an LLM (local Ollama by default, or
   an OpenAI-compatible API via `--api-base`) that classifies the space and
   identifies gaps (or, with `--fast`, derives the level straight from the
   similarity data)
5. **Output** — interactive TUI (default) or structured JSON (`--json`)

The embedding model loads concurrently with source searches, so the model-load
latency is hidden behind network I/O.

## The integrity rule

`patent` can prove something **exists**. It can **never** prove something
*doesn't* — it only searched some sources. Every verdict is scoped to "what was
found in the sources checked," and a clean result means *keep looking*, not
*start building*.

The sources-checked list is always displayed for transparency, and sources that
were selected but failed to respond are surfaced as "not reached" — so a thin
result is never mistaken for "nothing out there."

## Architecture

```
src/
├── lib.rs              # library root
├── model.rs            # Query, Match, Source, Verdict, Saturation
├── sources/
│   ├── mod.rs          # trait Source, search_all fan-out, dedup, retry
│   ├── crates_io.rs    # crates.io API
│   ├── github.rs       # GitHub search API
│   ├── npm.rs          # npm registry API
│   ├── pypi.rs         # PyPI search (HTML scraping)
│   ├── hacker_news.rs  # HN Algolia API
│   ├── go.rs           # Go package search
│   ├── maven.rs        # Maven Central API
│   ├── nuget.rs        # NuGet API
│   ├── rubygems.rs     # RubyGems API
│   ├── docker_hub.rs   # Docker Hub API
│   └── vscode.rs       # VS Code Marketplace API
├── rank.rs             # fastembed embeddings + cosine similarity
├── verdict.rs          # LLM prompt + response parsing
├── llm.rs              # trait Llm (verdict backend interface)
├── ollama.rs           # Llm impl: native Ollama client
├── openai.rs           # Llm impl: OpenAI-compatible chat client
└── tui.rs              # TUI state machine
src/bin/patent/
├── main.rs             # CLI entry point, pipeline wiring
├── cli.rs              # clap argument parsing
└── tui.rs              # ratatui rendering + event loop
```

Lib/bin split: the testable core is the library; the binary is a thin CLI/TUI
shell.

## Development

```bash
cargo fmt --all --check       # formatting (CI-enforced)
cargo clippy --all-targets -- -D warnings  # lint (CI-enforced)
cargo test                    # unit + wiremock integration tests
cargo build --release         # optimized build
```

The README demo GIF is generated with [vhs](https://github.com/charmbracelet/vhs):

```bash
vhs demo.tape                 # writes demo.gif
```

### Adding a new source

1. Create `src/sources/your_source.rs` implementing the `SourceAdapter` trait
2. Add the variant to `Source` in `src/model.rs`
3. Register it in `sources::build_source` and `sources::detect_sources`
4. Add wiremock integration tests in `tests/sources.rs`

## License

Licensed under either of [MIT](LICENSE-MIT) or
[Apache-2.0](LICENSE-APACHE) at your option.
