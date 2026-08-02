<p align="center">
  <img src="https://raw.githubusercontent.com/r14dd/patent/main/.github/logo-light.svg" width="600" alt="patent">
</p>

# patent

<p align="center">
  <a href="https://github.com/r14dd/patent/actions/workflows/ci.yml"><img src="https://github.com/r14dd/patent/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="https://crates.io/crates/patent"><img src="https://img.shields.io/crates/v/patent.svg?logo=rust" alt="crates.io"></a>
  <a href="https://docs.rs/patent"><img src="https://docs.rs/patent/badge.svg" alt="docs.rs"></a>
  <a href="#license"><img src="https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg" alt="license"></a>
  <a href="https://www.rust-lang.org"><img src="https://img.shields.io/badge/MSRV-1.88%2B-lightgray.svg?logo=rust" alt="MSRV"></a>
  <a href="https://ratatui.rs/"><img src="https://ratatui.rs/built-with-ratatui/badge.svg" alt="Built With Ratatui"></a>
</p>

`patent` takes a plain-English dev-tool idea and searches 16 sources — package registries (crates.io, npm, PyPI, Homebrew, Packagist, Hex, and more) plus GitHub and Hacker News. Results are ranked by semantic similarity and summarised as **Open**, **Crowded**, or **Saturated**.

<p align="center">
  <img src="https://raw.githubusercontent.com/r14dd/patent/main/showcase.gif" alt="patent demo" width="720">
</p>

> Like a patent search, but for code. It finds prior art, yet, never certifies absence.

## Why patent?

Before you build a dev tool, `patent` checks whether it already exists. One query fans out across 16 sources at once — package registries plus GitHub and Hacker News — instead of you searching each by hand. Matches are ranked locally by *semantic* similarity (not keyword match), so close-but-differently-worded prior art still surfaces, and the verdict is scoped to what was actually found — an honest "keep looking" rather than a hallucinated "this is novel."

## How it works

1. **Source selection** — `patent` picks the registries relevant to your idea; GitHub and Hacker News are always searched.
2. **Semantic ranking** — every match is embedded locally with [`fastembed`](https://github.com/Anush008/fastembed-rs) and ranked by cosine similarity to your idea.
3. **Maintenance signal** — where a source publishes one, a match shows when it was last updated, and anything untouched for 2+ years is flagged `⚑`. Stale matches are flagged, never demoted: an abandoned tool is still proof the idea has been built. A match with no date is one no source published a date for — not a stale one. (If two sources return the same URL, the surviving row borrows whichever date was found.)
4. **Verdict** — an LLM summarises the landscape into one of three levels, *floored* against the similarity data so it can never under-rate a populated space:
   - 🟢 **Open** — nothing close found in the sources checked.
   - 🟡 **Crowded** — a few adjacent tools exist.
   - 🔴 **Saturated** — the space is densely populated.

## What a clean result means

`patent` can prove something **exists**; it can never prove something **doesn't** — it only searched some sources. Every verdict is scoped to "found in the sources checked," the list of sources checked is always shown, and any selected source that failed is surfaced as "not reached." A clean **Open** result means *keep looking before you commit*, not a green light.

## Install

```bash
cargo install patent
```

Pre-built binaries are on the [releases page](https://github.com/r14dd/patent/releases).

**Ollama** (optional) powers the LLM verdict — install from [ollama.com](https://ollama.com), then `ollama pull qwen2.5`. Use `--fast` to skip it entirely.

**GitHub token** (optional) — set `GITHUB_TOKEN` to raise the search rate limit from 10 to 30 requests/minute.

**Linux build deps** — needed before `cargo install`:
- Fedora / RHEL: `sudo dnf install openssl-devel gcc-c++`
- Ubuntu / Debian: `sudo apt install libssl-dev g++`

**glibc 2.38+** — both the prebuilt binaries and a from-source `cargo install` require glibc 2.38 or newer (Ubuntu 22.10+, Debian 12+, Fedora 38+). The bundled ONNX Runtime that powers local semantic search depends on it. On older distributions such as Ubuntu 22.04 (glibc 2.35), build inside a newer toolchain — e.g. a `debian:12` / `ubuntu:24.04` container — rather than on the host ([#37](https://github.com/r14dd/patent/issues/37)).

## Usage

```bash
patent "interactive cli to kill whatever's on a port"
```

```bash
# interactive mode — launches a search prompt inside the TUI
patent

# no model warmup, no wait
patent "kubernetes log viewer" --fast

# pipe to jq
patent "react component for infinite scroll" --json | jq .

# use a cloud LLM instead of local Ollama
patent "kubernetes log viewer" --api-base https://api.openai.com/v1 --model gpt-4o-mini
```

## Options

| Flag | Description | Default |
|---|---|---|
| `--fast` | no LLM, no wait — verdict derived from similarity scores | — |
| `--json` | stdout JSON instead of the TUI | — |
| `--model <MODEL>` | model name for the verdict; or `PATENT_MODEL` | `qwen2.5` |
| `--api-base <URL>` | OpenAI-compatible base URL (must end in `/v1`); or `PATENT_API_BASE` | — |
| `--api-key <KEY>` | API key for `--api-base`; or `PATENT_API_KEY` / `OPENAI_API_KEY` | — |
| `--limit <N>` | max matches to keep after ranking | `50` |
| `--completions <SHELL>` | print shell completions and exit | — |

Settings can also be stored in a `config.toml` in your platform's config directory:

- **Linux**: `~/.config/patent/config.toml`
- **macOS**: `~/Library/Application Support/patent/config.toml`
- **Windows**: `%APPDATA%\patent\config.toml`

```toml
model    = "gpt-4o-mini"
api_base = "https://api.openai.com/v1"
api_key  = "sk-..."
```

Precedence: CLI flag > environment variable > config file > built-in default.

> **Privacy:** with `--api-base`, your search query is sent to that remote server to generate the verdict. The default local Ollama backend — and `--fast`, which skips the LLM entirely — keep everything on your machine.

## Exit codes

`patent` exits with a status derived from the verdict, so you can gate scripts and CI on it:

| Code | Meaning |
|---|---|
| `0` | Open |
| `1` | Crowded |
| `2` | Saturated |

(Usage errors and unreachable backends exit non-zero as well.)

## Use as a library

`patent` is also a published library crate — the engine is reusable. See [docs.rs/patent](https://docs.rs/patent): `sources::search_all` fans out to the registries, `rank::rank_async` ranks matches by semantic similarity, and `verdict::assess` produces the integrity-scoped verdict over any LLM backend.

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
| `Enter` | Show match details (description, popularity, last updated, URL) |
| `o` | Open selected URL in browser |
| `y` | Copy selected URL to clipboard |
| `n` | New search (interactive mode) |
| `?` | Help overlay |
| `q` | Quit |

### Markers

| Marker | Meaning |
|---|---|
| `⚑` | No update in 2+ years — only ever shown where a date is actually known, so its absence is not a claim of freshness |

Mouse works too — scroll with the wheel, click to select.

## Shell completions

```bash
patent --completions bash >> ~/.bashrc    # Bash
patent --completions zsh  >> ~/.zshrc     # Zsh
patent --completions fish > ~/.config/fish/completions/patent.fish
```

## Contributing

Contributions are welcome! See [CONTRIBUTING.md](CONTRIBUTING.md) for setup and workflow. The [`good first issue`](https://github.com/r14dd/patent/labels/good%20first%20issue) and [`help wanted`](https://github.com/r14dd/patent/labels/help%20wanted) labels are a good starting point.

## Development

```bash
cargo test
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
```

The demo GIF embedded above (`showcase.gif`) is generated with [vhs](https://github.com/charmbracelet/vhs): `vhs showcase.tape`.

## License

Licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE) at your option.
