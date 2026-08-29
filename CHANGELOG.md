# Changelog

## [0.12.0]

### Changed

- **Default Ollama model is now `qwen3.5` (was `qwen2.5`).** Existing installs
  need `ollama pull qwen3.5` — without it the verdict call returns
  model-not-found and falls back to the no-LLM verdict (scoped and caveated as
  always, but less specific). Pass `--model qwen2.5` or set `PATENT_MODEL` to
  stay on the old default
- The Ollama request now sends `"think": false`. Thinking models spend the
  whole `num_predict` budget on a reasoning trace that Ollama returns in a
  separate field, leaving `response` empty — measured 512 tokens of thinking
  and no answer, vs 11 tokens with thinking off. Non-thinking models ignore the
  field. It needs an Ollama daemon that knows `think` (0.9.0+); older daemons
  are not tested against this

## [0.11.2]

### Security

- h2 bumped to 0.4.18 for RUSTSEC-2026-0258. It comes in under `reqwest`, so
  every source adapter sits on top of it — patched rather than waited out

### Changed

- fastembed 5 → 6. The ranking model stays pinned to `AllMiniLML6V2` and the
  on-disk cache layout is unchanged, so an existing model cache is reused as-is
  and upgrading does not re-download the ~80 MB model
- Dependency refresh: serde 1.0.229, time 0.3.55, thiserror 2.0.20,
  async-trait 0.1.92, futures 0.3.34, toml 1.1.4, serde_json 1.0.151,
  clap_complete 4.6.9, open 5.4.1

### Fixed

- **The Go source reported itself as failed whenever it simply found nothing.**
  A genuine pkg.go.dev "no matches" page is ~33 KB, so it always tripped the
  drift guard (`zero packages parsed from a non-trivial response`) and Go was
  listed as *not reached*. A source that answered "nothing found" is reached,
  and calling it a failure overstates how little the search actually covered.
  The two cases are now told apart by a marker that, across the queries probed
  live, pkg.go.dev rendered on every zero-result page and on no page that had
  results; real markup drift is still reported as drift
- pkg.go.dev ANDs every search term, so a whole idea passed through as-is can
  return nothing even where matching packages exist — measured live, a 7-term
  idea returned zero results while 2–3 of its longest terms returned over a
  hundred. (Not every long query is affected: a 5-term idea whose terms are all
  common in the target's docs still returned plenty.) The adapter now
  progressively narrows to the 3 then 2 longest terms before giving up, the
  same fix the JetBrains adapter got in 0.11.0 for the same underlying
  behaviour. Where it bit, Go was contributing nothing to a search while being
  listed among the sources checked
- Go package names came through with the module path glued on, newlines and
  indentation included (`httprouter\n  (github.com/julienschmidt/httprouter)`),
  because the title anchor nests the path in a child span and the scrape walked
  all descendants. Only the anchor's own text is read now

### Note on exit codes

- Because Go now returns results where it previously returned none, a query
  whose prior art sits mainly in the Go ecosystem can land on a higher
  saturation level than it did in 0.11.1 — and so on a different exit code
  (`0` open / `1` crowded / `2` saturated). The contract is unchanged; the
  input to it is simply less incomplete. Worth knowing if you script on it

## [0.11.1]

### Changed

- Dependency refresh: tokio 1.53, anyhow 1.0.104, thiserror 2.0.19,
  futures 0.3.33, async-trait 0.1.91

### Fixed

- README now states the actual minimum Debian requirement for the prebuilt
  binary (glibc 2.38, i.e. Debian 13/trixie) — thanks @MalTeeez

## [0.11.0]

### Added

- **JetBrains Marketplace as a source**, the 19th. IDE-plugin ideas previously
  only reached the VS Code Marketplace, so anything already shipped for
  IntelliJ, PyCharm, GoLand, WebStorm or Rider was invisible to a search that
  should obviously have found it. Selected for the same `ide` / `plugin` /
  `extension` / `editor` signals as VS Code, plus the IDE names themselves
  (`intellij`, `pycharm`, `webstorm`, `goland`, `rider`, `clion`, `datagrip`,
  `phpstorm`, `rubymine`, `android studio`), and addressable directly as
  `--sources jetbrains` (`intellij` also works)
- JetBrains reports a last-updated date (`cdate`), making it the 9th of 19
  sources to do so, so its matches carry an age and can be flagged `⚑` like any
  other. The date arrives in the same search response the adapter already
  fetches, so it costs no extra request

### Fixed

- `searchPlugins` ANDs every content word in the query, so passing a whole idea
  through returned nothing at all: a 5-to-7-term idea reliably came back with
  `total: 0` even where a matching plugin existed. The adapter now searches on
  keywords and, when a query comes back empty, progressively narrows to the 3
  then 2 longest terms before giving up — measured against the live API, that
  recovers results for the queries the full set loses. Without it the source
  would have been listed as searched while being structurally incapable of
  returning anything, which reads as "nothing out there" rather than "nothing
  found"
- The documented source count said 16 in the README and the crate docs; it had
  actually been 18 since 0.9.0. Now 19 and correct in every place that states it
- The date table in `src/sources/mod.rs` listed neither half for the VS Code
  Marketplace, which reports no date. It is now named among the sources that
  return none, so the table again accounts for every source

### Breaking (library API)

- `Source` gained a `JetBrains` variant. The enum is not `#[non_exhaustive]`,
  so any downstream exhaustive `match` on `Source` needs a new arm. As with the
  `Match::last_updated` addition in 0.10.0, the break is documented rather than
  papered over with a wider one

## [0.10.0] - 2026-08-02

### Added

- **Maintenance signal on matches.** Where a source publishes a last-updated
  date, matches now carry one. The detail view reads `updated 3 years ago`, and
  anything untouched for two years or more is flagged `⚑` in the results table
  — flagged, never demoted: an unmaintained tool that exists is still prior art,
  so it keeps its rank and only gains a marker
- 8 of the 18 sources report a date in the same search response the adapter
  already fetches, so nothing costs an extra request: crates.io (`updated_at`),
  npm (`package.date`), GitHub (`pushed_at`), Hex (`updated_at`), Maven
  (`timestamp`), AUR (`LastModified`), Artifact Hub (`ts`), and pkg.go.dev
  (scraped from its rendered publication date). The remaining sources publish
  no date in their search results, and fetching one per match would mean a
  request per row that the per-source timeout has no room for; those matches
  show no date at all, which is *not* a claim that they are stale — unless
  another source returned the same URL, in which case dedup fills the gap and
  the row shows that source's date (see Fixed, below). Hacker News
  is left blank deliberately — it exposes when a thread was posted, which says
  nothing about whether the thing discussed is still maintained.
  `src/sources/mod.rs` carries the full table, so the gaps read as decided
  rather than unfinished
- `--json` output gained a `last_updated` key on every match: a whole-second
  RFC 3339 UTC timestamp, or `null` where the source reports none.
  `schema_version` stays `1` — the change is purely additive, so existing
  consumers keep working untouched
- The LLM prompt now shows each match's age, with explicit guidance to treat it
  as colour on the prior art and never as a reason to discount it: a tool that
  exists but looks unmaintained is still proof the idea has been built, and an
  absent date is not evidence of staleness
- `patent::freshness`: normalises every registry's date format — RFC 3339 at
  any sub-second precision, epoch seconds, epoch milliseconds, and
  pkg.go.dev's rendered `Feb 28, 2026` — to one canonical shape, then turns it
  into a display label and a staleness flag. Every constructor returns `Option`
  rather than `Result`, so a registry that changes its date format degrades
  that match to "no date known" instead of taking the whole source down. Date
  fields are also deserialised leniently: a value whose *type* changed upstream
  would otherwise abort the entire response and put the source dark
- The `⚑` marker is documented in the TUI help overlay (`?`) and in the README

### Changed

- The absence-claim scrubber now also catches absence dressed up as
  maintenance. Showing the model dates opens a second way to make the one claim
  this tool must never make — not "nothing exists" but "no actively maintained
  tool exists", which is just as unprovable: only 8 of the 18 sources publish a
  date at all, so an undated match is not an unmaintained one. Such headlines
  and gaps are now replaced or dropped like any other absence claim, and hedges
  ("no *currently* maintained tool") no longer walk one past the filter. Saying
  a *specific* match looks unmaintained is untouched — that is read straight off
  the data shown, and is the whole point of the signal
- Ranking is deliberately left similarity-only; recency is **not** blended into
  the score. `verdict.rs` derives the saturation floor from the similarity
  data, so deflating similarity for age would quietly weaken the integrity
  guard itself and let a well-trodden space read as "Open". Age belongs in the
  output, where a human and the model can weigh it — the rationale is recorded
  on `rank::score_sort_limit` so it is not silently undone later

### Fixed

- Dedup no longer discards what a duplicate knew. When two sources return the
  same URL — routinely, since 35% of Homebrew formulae have a homepage
  byte-identical to a GitHub repo URL — only the first arrival survived, and the
  fan-out iterates a `HashSet`, so which one that is is randomised per process.
  The same query really did show a last-updated date and a star count on one run
  and neither on the next. The kept match now fills its empty `last_updated` and
  `popularity` from the duplicates behind it; identity, ordering and every field
  it already had are untouched. This predates the maintenance signal — recency
  is simply the first field where losing the race became visible. `last_updated`
  and `popularity` are the whole of it: they are the only optional fields on
  `Match`. Which source is *credited* for a shared URL still varies per run, so
  the name and description shown are still whichever source won the race — that
  is untouched here, and no data is lost to it

### Breaking (library API)

- `Match` gained a public `last_updated: Option<String>` field. `Match` has no
  `Default` impl, so downstream code that constructs one with a struct literal
  must add `last_updated: None`. Reading and matching on existing fields is
  unaffected

## [0.9.1] - 2026-08-02

### Fixed

- **Nixpkgs returned nothing at all in 0.9.0.** The adapter sent no
  `Authorization` header, but the `search.nixos.org` Elasticsearch backend
  rejects anonymous callers, so every query failed with `401` and the source
  was always reported "not reached". It now sends the read-only credentials
  that `search.nixos.org` ships to browsers
- Nixpkgs queried index generation `latest-42-…`, which upstream has since
  deleted — that path `404`s even with valid credentials. Pinned to a live
  generation and pulled out into a named constant
- Nixpkgs matched nothing for any multi-word idea. Its Elasticsearch query was
  copied from the `search.nixos.org` frontend, where the input is a one- or
  two-word search box, and kept that frontend's `"operator": "and"` — so every
  extracted keyword had to match a single package. Now `"or"`, casting wide and
  leaving precision to the local ranker like every other source. Without this,
  fixing the `401` alone would have been the worse bug: a source reported as
  *reached* that always returns nothing reads as "no prior art" rather than
  being surfaced as "not reached"

### Added

- `live_nixpkgs` smoke test. Nixpkgs was the only source with no live-network
  coverage, which is why the failures above shipped behind a green suite — the
  mocked tests stub out the auth wall and the index name alike
- `live_nixpkgs_multi_keyword` smoke test, sending the kind of keyword list the
  real pipeline emits — a single-keyword probe passes even when the query is
  conjunctive, so it cannot catch the empty-results failure on its own
- A hermetic `nixpkgs_sends_authorization_header` test, so dropping the auth
  fails in PR CI instead of waiting for the nightly live run

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
