# Changelog

## [0.5.1] - 2026-06-23

### Fixed

- npm: decode `score.detail.popularity` from the search API response (was always `None`)
- homebrew: synthesize a stable `formulae.brew.sh/formula/{name}` URL when a formula has no
  homepage, so homepage-less entries no longer silently collapse in dedup
- sources: `dedup` now skips empty/whitespace URLs and falls back to a `(name, source)` key,
  preventing all blank-URL matches from collapsing into one
- pypi, go: return a parse error when a non-trivial response page yields zero results, so the
  retry path fires on markup drift instead of silently returning empty
- verdict: add 8 missing absence phrases to the integrity guard list; always substitute the
  data-derived headline when the level is Open and no close match exists
- rank: `model_is_cached()` recursively checks for a non-zero `.onnx` file so a partial
  download no longer suppresses the first-run progress notice
- main: config loading moved before the interactive TUI early-return so the TUI picks up the
  configured LLM backend; `--limit` now ranks with `max(limit, DEFAULT_LIMIT)` for verdict
  quality and truncates to the requested limit for display after
- main: replace `.expect()` panics on spawn tasks with proper error propagation
- tui: `execute_pipeline` now calls `verdict::assess` with the configured LLM backend
  (falling back to `from_data` on error) instead of always using the no-LLM path; uses
  `DEFAULT_LIMIT` instead of a hardcoded 30

## [0.5.0] - 2026-06-20

### Added

- Interactive TUI search mode: launch `patent` with no arguments to get a search prompt,
  spinner, and results viewer without leaving the terminal
- Homebrew source: searches formulae and casks from `formulae.brew.sh`
- Config file (`~/.config/patent/config.toml`): set `api_base`, `api_key`, and `model`
  persistently; CLI flags and environment variables take precedence
- `n` keybinding in results view to start a new search without quitting

### Fixed

- Block-on replaced with async/await in the TUI pipeline to prevent tokio runtime panics

## [0.4.0] - 2026-06-09

### Added

- Clipboard copy with `y` in the results view
- Exit codes: 0 for Open, 1 for Crowded/Saturated, 2 for errors
- Docker Hub source
- Nix flake

### Fixed

- Maven: stop publishing `versionCount` as a popularity signal
- crates.io: honor `self.base_url` when building result URLs
- Various search recall improvements

## [0.3.0] - 2026-06-06

### Added

- Redesigned detail popup with score-colored border, compact metadata, and scrollable
  description
- Showcase recording in README

### Fixed

- Search recall, verdict quality, and input validation
- Various bug fixes and recall improvements

## [0.2.0] - 2026-06-03

### Added

- OpenAI-compatible LLM backend (`--api-base`, `--api-key`, `--model`)
- Cross-platform release binaries and shell installer via cargo-dist
- Intra-doc links and crate-level documentation

### Fixed

- Verdict on close matches

## [0.1.0] - 2026-06-02

Initial release.

- Prior-art search across 11 sources: crates.io, npm, PyPI, GitHub, Go, Maven, NuGet,
  RubyGems, Docker Hub, VS Code Marketplace, and Hacker News
- Local semantic ranking with `fastembed` (AllMiniLML6V2)
- LLM verdict via local Ollama, with `--fast` mode for similarity-only results
- `ratatui` TUI: scrollable match table, filter, sort, detail popup, mouse support
- `--json` output and exit codes
- Verdict integrity guards: `floor_level`, `guard_headline`, absence-phrase scrubbing,
  sources-checked transparency, fixed humble caveat
