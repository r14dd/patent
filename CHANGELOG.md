# Changelog

## [0.9.0] - 2026-07-16

### Added

- New source: **Nixpkgs** (NixOS/Nix) — searches the NixOS package index via
  its Elasticsearch backend. 18 sources total
- `--sources` flag: restrict search to a comma-separated list of sources
  (e.g. `--sources npm,pypi,crates-io`)
- `--exclude` flag: skip specific sources from the search fan-out
  (e.g. `--exclude hacker-news,maven`)
- `--list-sources` flag: print every known source and exit
- `--print-prompt` flag: run the search pipeline in fast mode and print the
  raw LLM prompt to stdout (useful for piping into other models)
- `Source::FromStr` implementation with human-friendly aliases (`docker` for
  docker-hub, `brew` for homebrew, `nix` for nixpkgs, `vscode` for
  vscode-marketplace, etc.)
- Library API: `sources::search_filtered()` for explicit include/exclude
  source control
- Library API: `Source::all()` and `Source::kebab_name()` for programmatic
  source enumeration

### Breaking (library API)

- `Source` enum gained `Nixpkgs` variant
- New public functions on `Source`: `all()`, `kebab_name()`, `FromStr` impl

## [0.8.0] - 2026-07-07

### Added

- New source: **Hackage** (Haskell) — searches the package archive and
  batch-fetches cabal files for synopsis/homepage (#40). 17 sources total
- `--keyword-only` flag: rank by keyword overlap instead of semantic similarity,
  skipping the ~80 MB embedding model download entirely (#30)
- Shared search→rank→verdict pipeline (`pipeline.rs`) used by both CLI and TUI,
  fixing TUI bugs (missing relevance gate, wrong eval limit) (#63)
- Stable `--json` output: `schema_version` field, `Source`/`Saturation` serialize
  to kebab-case/lowercase with backward-compatible aliases (#61)
- Per-source 15 s wall-clock timeout prevents a single slow source from blocking
  the entire fan-out (#33)
- Homebrew source caches formula.json/cask.json in memory — repeated searches
  in the TUI reuse the catalog instead of re-downloading ~10 MB (#45)
- dist: Homebrew tap installer (`brew install r14dd/patent/patent`) and
  PowerShell installer for Windows (#35)
- ~~dist: Windows prebuilt binary~~ — reverted; ort-sys static linking still
  has 43 unresolved MSVC externals. Windows users install via
  `cargo install patent` (#36)

### Changed

- pypi: PyPI's search failure is now surfaced accurately. Its search page is
  bot-walled to non-browser clients (a `403`, or a `200` JS-challenge stub with
  no results) — that now returns honest wording ("PyPI search is unavailable to
  non-browser clients") instead of the misleading "search page structure may have
  changed" parse error. PyPI is still reported as *not reached*, never as an
  empty result.
- sources: the fan-out no longer wastes its one 800ms retry on a persistently
  unavailable source — only transient failures (network blips, HTML parse drift)
  are retried; a walled search surface is attempted once.

### Breaking (library API)

- New `Error::Unavailable(String)` variant (downstream exhaustive matches on
  `patent::Error` must update)
- `Source` enum gained `Hackage` variant
- `rank::rank_by_keywords` added to the public API

## [0.7.0] - 2026-07-02

### Added

- New sources: **Packagist** (PHP/Composer), **Hex** (Erlang/Elixir), **Artifact
  Hub** (Cloud Native — Helm charts, operators, kubectl plugins), and **AUR**
  (Arch User Repository) — 16 sources total (#24, #41, #42, #22)
- verdict: the LLM prompt now includes each match's popularity and URL so the
  model can weight firmly-established prior art (#60)
- tests: a live smoke-test harness (`tests/live.rs`) that exercises every source
  against its real API — the one failure mode the hermetic wiremock suite can't
  catch (an upstream response shape changing while the mocked tests stay green).
  All tests are `#[ignore]`d so `cargo test` stays offline; a nightly `live`
  workflow runs them on a schedule so drift surfaces as a failed run

### Changed

- **Privacy:** running with `--api-base` now prints a one-time notice that the
  search query is sent to the remote server; also documented in the README (#62)
- **Breaking (library API):** the `Source` enum gained `Packagist`, `Hex`,
  `ArtifactHub`, and `Aur` variants (downstream exhaustive matches must update)

### Known issues

- **PyPI search is temporarily unavailable.** PyPI has retired its keyless search
  paths (the XML-RPC endpoint is gone) and now serves its web search behind a bot
  challenge, so the scrape-based adapter returns nothing. This is surfaced
  honestly at runtime — PyPI is reported as *"not reached,"* never as an empty
  result — and the live smoke test for it is skipped in CI pending a keyless
  replacement backend, tracked for 0.7.1

## [0.6.0] - 2026-06-30

### Added

- rank: `rank_async` — an async-safe sibling of `rank` that offloads the blocking,
  CPU-bound `fastembed` work onto `tokio::task::spawn_blocking` so it never stalls
  the executor when called from async code (#34)
- github: searches now use GitHub's default best-match (relevance) ordering instead
  of `sort=stars`, with a wider result page, so low-star but on-topic repositories
  surface for the semantic ranker instead of being buried by popular-but-unrelated
  ones (#31)
- ci: cache the `fastembed` embedding model across runs, removing the flaky
  per-run model download (#46)

### Changed

- **Breaking (library API):** the HTTP-client builders no longer panic on failure.
  `sources::search_all` now returns `Result<SearchOutcome>`, and `Ollama::new` /
  `OpenAi::new` now return `Result<Self>`; a new `Error::HttpClient` variant carries
  the build failure. `impl Default for Ollama` was removed (it could no longer be
  infallible and had no callers). The `patent` binary is unaffected — it surfaces
  the error via `anyhow` instead of aborting on a panic (#32)
- rank: `rank` now documents that it is blocking and points async callers to
  `rank_async`
- **MSRV raised to 1.88** to match the actual minimum required by dependencies
  (`ratatui`, `ort`, `image`, `time`); the previous `1.80` claim was already
  unmet by the locked dependency tree. A CI job now enforces it.

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
