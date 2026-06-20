<p align="center">
  <img src="https://raw.githubusercontent.com/r14dd/patent/main/.github/logo-light.svg" width="600" alt="patent">
</p>

# patent

<p align="center">
  <a href="https://github.com/r14dd/patent/actions/workflows/ci.yml"><img src="https://github.com/r14dd/patent/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="https://crates.io/crates/patent"><img src="https://img.shields.io/crates/v/patent.svg?logo=rust" alt="crates.io"></a>
  <a href="https://docs.rs/patent"><img src="https://docs.rs/patent/badge.svg" alt="docs.rs"></a>
  <a href="#license"><img src="https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg" alt="license"></a>
  <a href="https://www.rust-lang.org"><img src="https://img.shields.io/badge/MSRV-1.80%2B-lightgray.svg?logo=rust" alt="MSRV"></a>
  <a href="https://ratatui.rs/"><img src="https://ratatui.rs/built-with-ratatui/badge.svg" alt="Built With Ratatui"></a>
</p>

`patent` takes a plain-English dev-tool idea and searches 12 open-source registries — crates.io, npm, PyPI, GitHub, Homebrew, and more. Results are ranked by semantic similarity and summarised as **Open**, **Crowded**, or **Saturated**.

<p align="center">
  <img src="https://raw.githubusercontent.com/r14dd/patent/main/showcase.gif" alt="patent demo" width="720">
</p>

> Like a patent search, but for code. It finds prior art, yet, never certifies absence.

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

Settings can also be stored in `~/.config/patent/config.toml`:

```toml
model    = "gpt-4o-mini"
api_base = "https://api.openai.com/v1"
api_key  = "sk-..."
```

Precedence: CLI flag > environment variable > config file > built-in default.

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
| `Enter` | Show match details (description, popularity, URL) |
| `o` | Open selected URL in browser |
| `y` | Copy selected URL to clipboard |
| `n` | New search (interactive mode) |
| `?` | Help overlay |
| `q` | Quit |

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

The demo GIF is generated with [vhs](https://github.com/charmbracelet/vhs): `vhs demo.tape`.

## License

Licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE) at your option.
