# patent 0.6.0 — integrity fixes, richer data, sharper verdicts

_Local notes only (untracked). The core theme: stop discarding signal that
the APIs already return, fix the correctness gaps that undermine the product
promise, and raise the quality ceiling for recall, ranking, and verdicts._

Issues marked `(#N)` already exist on GitHub. Items without a number are new
findings from the 0.6.0 planning pass.

---

## ⚠️ Integrity / correctness bugs (fix before release)

These undermine the tool's core promise. They are bugs, not enhancements.

### TUI silently hands out the no-LLM verdict (issue #44)
`src/bin/patent/tui.rs:1150` — `execute_pipeline` calls `verdict::from_data`
unconditionally, so the **default UX** (running `patent` with no args) always
gets the flooring-only verdict: no gaps, no model headline, no LLM reasoning.
The `--json`/CLI path correctly calls `verdict::assess`. Also: `execute_pipeline`
hardcodes `limit = 30`, ignoring `--limit` and config.

Thread the resolved LLM backend and config limit into `run_interactive`, call
`verdict::assess` with the same fallbacks as `main.rs:224-276`. At minimum,
render a visible `[similarity-only — LLM not run]` label so the degraded result
is never mistaken for a full one. The architectural fix (shared pipeline, below)
is the clean way to prevent this recurring.

### Blank Homebrew URLs silently collapse distinct tools in dedup
`src/sources/homebrew.rs:123` — `url: pkg.homepage.unwrap_or_default()` writes
`""` for formulae without a homepage. `dedup` in `sources/mod.rs:319-325` keys
on URL via a `HashSet`, so the first empty-URL formula claims `""` and every
subsequent one is silently dropped as a "duplicate" — real distinct prior art
disappears with no warning.

Fix in two steps: (1) in `dedup`, never treat an empty/whitespace URL as a
dedup key — fall through to a `(name, source)` key; (2) in the Homebrew adapter,
synthesize `https://formulae.brew.sh/formula/{name}` when `homepage` is `None`
so every row is unique and clickable.

### PyPI / Go HTML scrapers return `Ok(empty)` on selector drift
`src/sources/pypi.rs:60-105`, `src/sources/go.rs:54-103` — both scrape live
HTML via CSS selectors. When upstream markup changes, `document.select` yields
nothing and the adapter returns `Ok(vec![])` — indistinguishable from genuine
"no prior art", so a populated space reads as Open: a direct integrity violation.
Go is extra-fragile, grabbing the first `a[href]` which can be a non-name link.

Fix: when the page body is non-trivial in size but zero snippets parse, return
`Err` (→ reported "not reached") rather than `Ok(empty)`. Add parser unit tests
against saved fixture HTML so CI catches stale selectors. Long term, prefer JSON
endpoints (PyPI JSON API at `https://pypi.org/search/?q=&o=&c=`, pkg.go.dev).

### `--limit` weakens the integrity floor
`src/rank.rs:40`, `src/verdict.rs:233-236` — `floor_level` and the prompt's
`strong`/`close` counts iterate the *post-truncation* matches slice, and
`args.limit` wires directly into `rank`'s truncation. So `--limit 5` shrinks
the set the saturation count is computed over and can under-rate a saturated
space — the integrity floor is undermined by the display flag.

Fix: count saturation over a fixed evaluation window (top-50, or the full scored
set) regardless of the user's display `--limit`. Pass the full slice to
`floor_level` / `build_prompt`, then truncate only for display.

### `guard_headline` missing novelty-claim phrasings
`src/verdict.rs:122-163` — `ABSENCE_PHRASES` misses common model outputs that
assert novelty/absence: `"greenfield"`, `"wide open"`, `"untapped"`,
`"we found zero"`, `"zero implementations"`, `"gap in the market"`,
`"no one is solving"`. Add these. Also: when level is `Open` **and** the
close-match count is 0, always substitute `data_headline` rather than trusting
any model headline — that combination is exactly where an absence leak is
most likely and most damaging.

### `sources_failed` never influences verdict level
`src/verdict.rs:393-410` — `sources_failed` is displayed (good) but never
passed to `build_prompt` and never factors into `floor_level` or `from_data`.
An `Open` verdict over 2-of-8 reached sources reads identically to one over
8-of-8. Tell the prompt the failed-source count and instruct more caution under
reduced coverage; at minimum append a coverage qualifier to the headline when
`sources_failed` is non-empty and level is `Open`.

---

## Match model — extend with signal APIs already return

### Add `repository`, `last_updated`, `license`, `archived` to `Match`
`src/model.rs:55-66` — the keystone change. The same HTTP payloads already
parsed by each adapter contain this data; nothing extra is fetched.

Fields to add (all `Option<T>`, `#[serde(default)]` so `--json` stays additive):
- `repository: Option<String>` — canonical source repo URL
- `last_updated: Option<String>` — ISO 8601 / RFC 3339
- `license: Option<String>`
- `archived: Option<bool>`

Per-adapter sources: crates.io `repository`/`updated_at`, npm
`links.repository`/`time.modified`, GitHub `pushed_at`/`archived`/`license`,
RubyGems `source_code_uri`/`updated_at`, Homebrew `tap`. Render in TUI detail
+ JSON. The recency/staleness half overlaps issue #29.

### Capture npm quality score as `popularity` instead of `None`
`src/sources/npm.rs:75` — npm's `/-/v1/search` response includes
`score.detail.popularity` (0.0–1.0) per result, but the adapter hardcodes
`popularity: None`. npm is an always-on fallback registry and the flagship
port-killer demo — being the only major source with no popularity means npm
rows sort to the bottom and give the verdict no adoption signal. Smallest fix:
decode `score.detail.popularity` and set
`popularity = (detail * 1_000_000) as u64`.

### Surface staleness / archived badge in TUI and verdict (issue #29)
Once `last_updated`/`archived` exist on `Match`, show a relative-age badge in
the TUI row/detail and feed staleness into the verdict prompt. A space full of
abandonware is "Open for a maintained alternative" — a signal the current
similarity + count logic cannot express.

---

## Recall — single-query, per_page=20, closed allow-list misses prior art

### Raise `per_page` and fan out query variants (issue #31)
Every adapter issues one query capped at 20 (`per_page`/`size`/`rows`/`take`
all `= 20`), but the ranker truncates to `DEFAULT_LIMIT = 50` anyway — 20 is
an artificially low ceiling. Raise to ~50. Also issue a small fan of query
variants per source (full idea, top-2-keyword pairs, individual high-salience
keywords) and union before dedup+rank; the embedder re-ranks, so
low-precision recall is cheap.

### GitHub: switch to `best-match`, stop dropping description-less repos (issue #31)
`src/sources/github.rs:82,110-111` — search is fixed to `sort=stars per_page=20`,
which buries niche/new repos. The adapter also drops every repo with an empty
description, discarding code-only matches whose README/topics carry the real
signal. Use `sort=best-match` (or run two queries and union); fall back to repo
topics/name when description is empty rather than dropping the row.

### Homebrew: ANY-keyword matching and ETag-cached catalog (issue #45)
`src/sources/homebrew.rs:67-128` — currently fetches the entire multi-MB
`formula.json` + `cask.json` on every query under the shared 10s timeout, then
requires **all** keywords as substrings. "Kill port process" misses a formula
described "reclaim a port." Switch to any-keyword/token-overlap matching (let
the embedder re-rank), and ETag-cache the catalog between invocations.

### Broaden `detect_sources` or pick sources semantically
`src/sources/mod.rs:74-236` — source selection is a closed hand-maintained
keyword allow-list. "PHP/Composer", "Elixir", "Haskell", "Terraform" ideas all
fall into the no-signal fallback (npm/PyPI/crates) that doesn't contain their
prior art — silent under-coverage. Broaden the fallback to include more
language-agnostic registries when confidence is low, and/or embed the idea
against short source descriptors to pick sources semantically. Surface the
chosen branch in `--json` so under-coverage is debuggable.

---

## Ranking — pure cosine ignores keyword, popularity, recency

### Blend lexical (BM25/keyword-overlap) with the semantic score
`src/rank.rs:31-41` — ranking is cosine-only. For short dev-tool queries with
distinctive tokens ("port killer", "OTLP", a specific CLI name), exact keyword
overlap is a strong precision signal that cosine underweights.
`Query.keywords` are already computed (`main.rs:88-98`) but used only for
source API calls, never ranking. Add a lexical score over `name + description`
and combine: `final = α·cosine + (1-α)·lexical_norm`. Reuses existing
keywords; lays the groundwork for the keyword-only no-ML mode (issue #30).

### Embed tags/topics alongside name + description
`src/rank.rs:115-129` — the embedding document is `"name: description"` (or
bare name when description is empty); thinly-described tools score low and get
truncated before anyone sees them. Add an optional `tags: Vec<String>` to
`Match`, populate from source-provided topics/keywords/categories, and
concatenate into the embedding document.

### Blend popularity as a tie-break in ranking
`src/rank.rs:31-41` — `Match.popularity` is captured but ranking sorts on
similarity alone, so a 3-star toy can outrank a 50M-download incumbent.
Apply a mild popularity tie-break/boost among near-equal-similarity matches
(log-normalised within-source, small additive term or secondary sort within a
similarity band). Overlaps issue #29 (recency). Avoid cross-source raw
comparisons — normalise per-source first.

---

## Verdict enrichment — the LLM is judging without key signals

### Pass `popularity` to the verdict prompt
`src/verdict.rs:57-62` — `build_prompt` emits each match as
`"- **name** (source, sim 0.62): description"` and never includes
`Match.popularity`, even though it's populated for most sources. The model is
asked to judge "little room for differentiation" (Saturated) with no idea
whether a tool has 50M downloads or 3 stars. Include popularity in the match
line with a per-source unit label (downloads/stars/points). Purely additive,
no integrity impact. **Effort S.**

### Let the verdict cite competitors by name + URL
`src/verdict.rs:57-62,97-110` — `Match.url` is never passed to the prompt, and
`Verdict` has no field for cited matches, so the headline can only say "4 tools
turned up" without naming or linking the strongest one. Add the top match
names + URLs to the prompt and a `closest_matches: Vec<{name, url, why}>` field
to `Verdict`, populated by matching model-cited names back to ranked matches
(keeps integrity — we only cite what we actually found).

### Add a `confidence` / coverage signal to the verdict
`src/verdict.rs:97-110`, `src/model.rs:107-120` — `Open` over 2-of-8 reached
sources reads the same as `Open` over 8-of-8. Add an optional `confidence`
enum (`low/medium/high`) derived from coverage (match count, fraction of
selected sources reached) and floor it against reduced coverage — analogous to
`floor_level`, staying within integrity.

### Ollama model-not-pulled preflight or sharper UX
`src/ollama.rs:76-101` — when the model isn't pulled, the 404 surfaces as
`LlmRejected` with a "Run ollama pull X" hint, but only **after** the full
search+rank+connect window. Add a cheap preflight (`GET /api/tags`) at startup
so the pull hint fires before the pipeline, and disambiguate server-down vs
model-missing in the error string.

---

## Architecture — factor shared pipeline

### One `run_pipeline` library function for TUI and CLI
`src/bin/patent/tui.rs:1126-1157`, `src/bin/patent/main.rs:170-204` —
`execute_pipeline` in the TUI duplicates `main.rs`'s search+rank+verdict wiring
and has already drifted (the TUI verdict bug and limit bug above). Factor one
async library function called by both `main.rs` and the TUI. This is the
structural fix that prevents the TUI verdict downgrade from recurring, makes
the pipeline testable, and is the prerequisite for threading LLM config into
the TUI cleanly.

### `Source`: add `FromStr` + kebab-case canonical name
`src/model.rs:19-52` — `Source` has only a human `Display` (`"crates.io"`,
`"VS Code"`) and no `FromStr`; `--sources`/`--exclude` (issue #27) need stable
machine-parseable kebab names (`crates-io`, `vs-code-marketplace`). Implement
`FromStr` and a kebab canonical name. Pairs with the JSON rename item below.
This is a prerequisite for issue #27.

### `Query::new()` in the library
`src/model.rs:12-16`, `src/bin/patent/main.rs:88-98` — keyword derivation
(lowercase, stopword/punctuation strip) lives in the binary; a library consumer
of `search_all` gets no keywords unless they re-implement it. Add
`Query::new(idea: &str) -> Query` in the library and have the binary call it.

---

## Output contract — `--json` is leaky and unversioned

### serde-rename `Source` to stable kebab-case IDs
`src/model.rs:19-33` — `Source` derives `Serialize` with no `rename_all`, so
`--json` emits `"CratesIo"`, `"VsCodeMarketplace"`, `"HackerNews"` — internal
Rust identifiers that differ from the human labels, match no documented
vocabulary, and break on a field rename. Add `#[serde(rename_all = "kebab-case")]`
(or per-variant renames) to emit a stable, documented id set. **Effort S.**

### Add `schema_version` and coverage meta to `--json`
`src/bin/patent/main.rs:292-297` — the payload has no version and omits data
the pipeline already computes: `keywords`, the relevance-gate decision (a
consumer can't tell "gated as irrelevant" from "genuinely empty"),
`verdict_source` (`llm|fast|fallback|low-relevance`), and `best_similarity`.
Add a top-level `schema_version: 1`, bump on breaking changes, and a small
`meta` block for the above. Keep additive.

---

## TUI polish

### No-color / accessibility mode
`src/bin/patent/tui.rs:39-83,242-272` — no `NO_COLOR`/`--no-color` handling.
Score coloring and saturation icons are the **only** signals distinguishing
strong vs weak matches; they have no text fallback and may not render in all
terminals. Honor `NO_COLOR` and/or add `--no-color` with a monochrome theme
(ASCII `[OPEN]`/`[CROWDED]`/`[SAT]`, score buckets as text).

### Empty-state message and distinct error state
`src/bin/patent/tui.rs:461-466` — when `displayed` is empty, the table renders
as a blank header with no guidance. A failed search dumps the error into the
verdict headline with zero matches, so a failed run looks like a successful
empty run. Render a centered placeholder branching on filter-active vs
genuinely-no-results, and style the failed state distinctly.

### Detail view: per-source unit label, license, recency
`src/bin/patent/tui.rs:617-647` — the detail popup renders `popularity`
uniformly as `N ★` even though it is stars (GitHub), downloads
(npm/PyPI/crates), or points (HN). No license or recency. Once `Match` carries
`license`/`last_updated` (data-enrichment), render them in the detail meta line
and replace `★` with a source-appropriate unit label.

### Search input: horizontal scrolling for long ideas
`src/bin/patent/tui.rs:162-181` — the search box clips text and pins the cursor
at `width-2`; for a full-sentence idea the user can't see what they're typing.
Implement a horizontal scroll window anchored on the cursor.

### Status message reliable repaint
`src/tui.rs:281-287`, `src/bin/patent/tui.rs:546-555` — the 2s-expiry status
message (e.g. "Copied to clipboard") can stay rendered past expiry in Results
mode because redraws happen only on input events. Poll on a short timeout when
a status message is active so the footer reliably repaints.

---

## Robustness

### Retry must skip 4xx errors
`src/sources/mod.rs:282-293` — the retry sleeps 800ms and retries any `Err`,
regardless of error class. A 401/404 (and GitHub's auth errors mapped to
`Error::Parse`) will fail identically on retry, wasting 800ms + a doomed
round-trip. Add an `is_retryable()` classifier: skip sleep+retry for
4xx/auth/parse, use exponential backoff for 429/503.

### Per-source wall-clock timeout (issue #33)
`src/sources/mod.rs:282-293` — one trickle-data source (or Homebrew's multi-MB
catalog) dominates `join_all` for ~20s. Wrap each per-source retry block in
`tokio::time::timeout(~15 s)` so one slow source can't dictate total latency.

### `model_is_cached()` false-positive after interrupted download
`src/rank.rs:68-76` — returns `true` whenever the cache dir is non-empty. An
interrupted ~80 MB download leaves a partial dir: the next run prints no
"downloading" notice and looks like a hang (the exact failure this function
exists to prevent). Check for the specific expected model artifact
(`.onnx` / `tokenizer.json` under the `AllMiniLML6V2` subdirectory) and verify
non-zero size before suppressing the notice.

### Spawned ranking tasks: `.expect()` → anyhow
`src/bin/patent/main.rs:184,204` — `ranker_result.expect("embedding task panicked")`
turns a `JoinError` into a raw panic instead of using the `anyhow` error path
used everywhere else. Map with `.map_err(|e| anyhow::anyhow!("ranking task failed: {e}"))?`.

### Fallible client constructors (issue #32)
`src/sources/mod.rs:50-51`, `src/ollama.rs:24-28`, `src/openai.rs:20-24` —
three `.expect("failed to build HTTP client")` calls panic on TLS init failure
in a published library crate. Change `Ollama::new`, `OpenAi::new`, and
`http_client` to return `crate::Result<Self>`.

### `OpenAi`: derive `Clone`, manual `Debug` redacting `api_key` (issue #19)
`src/openai.rs:10-16` — `Ollama` derives both; `OpenAi` derives neither and
holds a plaintext key in its fields. Add a manual `Debug` that renders
`api_key` as `Some("***")/None`.

### Validate `api_base` URL and reject empty config values
`src/bin/patent/main.rs:128-152` — a malformed `api_base` surfaces only after
search+rank have already run. Validate it parses as an `http`/`https` URL at
startup, and reject empty-string config values at load time.

### Async `rank()` or document the blocking footgun (issue #34)
`src/rank.rs:136-143` — the public `rank()` wrapper is synchronously
CPU-heavy; library consumers in async context block the executor. At minimum,
add a `# Panics / Blocking` doc comment. Better: add `rank_async()` that
internally `spawn_blocking`s, making the safe path the default.

---

## Test gaps

### Malformed-body tests for GitHub, npm, Hacker News (issue #14)
A 200 + HTML/garbage body from one source should surface as `Error::Http` and
land in `failed`, not abort the run. Add wiremock tests, plus a
`search_sources`-level test where one adapter fails while another succeeds.

### Empty-results tests for 6 sources (issue #15)
Go, Maven, NuGet, RubyGems, Docker Hub, VS Code Marketplace have no
empty-results coverage. Add empty-results tests; also an all-empty-description
test for NuGet and Docker Hub (which filter those rows).

### Retry-recovery test: success on second attempt (issue #16)
The retry branch that actually recovers on a transient failure has no test.
Use wiremock `up_to_n_times` to return 500 once then 200 and assert the source
lands in `reached` with results.

### Timeout behavior test (issue #17)
The HTTP 10s timeout has no test. Add a wiremock test with
`set_delay(30s)` asserting the source lands in `failed` rather than hanging.

### Offline / DNS-failure end-to-end test
No test covers zero-network behavior. Add an integration test pointing all
adapters at an unroutable/closed address, asserting `matches` empty,
`reached` empty, `failed` populated, and the binary exits with the scoped
fallback verdict.

### MSRV CI job at 1.80 (issue #18)
`Cargo.toml` declares `rust-version = "1.80"` but CI only tests stable. Add a
matrix entry with `dtolnay/rust-toolchain@1.80` running `cargo check --all-targets`.

---

## Privacy & trust (new category)

### Disclose that `--api-base` ships the raw idea off-machine
`src/verdict.rs:43`, `src/openai.rs:36-48` — `build_prompt` injects
`query.idea` verbatim into the prompt, which is POSTed to whatever `--api-base`
is configured (OpenAI/OpenRouter/Groq) with no disclosure, redaction, or
opt-out. A prior-art tool is precisely where users paste unreleased product
ideas. Add a visible warning in the help text for `--api-base` and a one-line
notice at runtime. Ollama / `--fast` are local-only — make that explicit too.

### `api_key` stored plaintext: file-permission check or keyring note
`src/bin/patent/config.rs:10-27` — `api_key = "sk-..."` is documented to go
in `~/.config/patent/config.toml` with no `0600` permission check, no
world-readable warning, and no support for a `*_FILE` / keyring indirection.
Add a permission check on load (warn if group/other-readable on Unix), and
document the `PATENT_API_KEY` env var as the preferred secret-passing mechanism.

### Affirmative "no telemetry" statement
No analytics/telemetry exists in the codebase (confirmed) — but for a tool
handling confidential ideas, the absence of an explicit statement reads as
silence. Add one sentence to the README: "No telemetry. All processing is local
except the optional `--api-base` LLM call."

---

## Observability

### Add `--verbose` / `RUST_LOG` tracing
There are ~15 ad-hoc `eprintln!` calls and no structured logging. No way to
see which `detect_sources` branch fired, the actual request URLs, per-source
latency/result counts, or the full prompt at runtime. Instrument the pipeline
with `tracing` (or the lighter `log` crate) and honor `RUST_LOG`/`--verbose`
so a thin result is diagnosable without rebuilding.

---

## Caching / offline

### Query-result cache (keyed on idea + sources, TTL'd)
Every run re-fans-out to all sources + re-embeds + re-calls the LLM with zero
memoization. A `(idea, sources)`-keyed cache in the platform cache dir (a
sibling of the fastembed model dir, `dirs::cache_dir()`) would eliminate
repeated identical queries, reduce upstream API load, and enable true offline
use for repeated ideas. TTL of ~1 hour covers interactive iteration; a
`--no-cache` flag overrides.

---

## Distribution

### Fix glibc + MSVC root cause via `ort` `load-dynamic` / `download-binaries` (issues #36, #37)
Both the Windows MSVC linker failure and the Ubuntu 22.04 glibc-floor failure
trace to the same root: ONNX Runtime prebuilt via `ort`/`fastembed`. Try
`ort`'s `load-dynamic` strategy (link at runtime, sidestepping the baked-in
glibc prebuilt) or the `download-binaries` feature (CI fetches a matching
runtime). Validate on a 22.04 container and `windows-msvc` runner. This likely
resolves Windows, Intel macOS, and the glibc floor in one change.

### Intel macOS: restore prebuilt or document gap
`dist-workspace.toml` ships `aarch64-apple-darwin` + Linux, but no
`x86_64-apple-darwin`. Intel Mac users fall back to `cargo install patent`
which fails on the same ONNX issue. Add `x86_64-apple-darwin` once
`ort`/`fastembed` provide a prebuilt, or surface a tested error with install
instructions.

### Harden `profile.dist` and document the ~80 MB model download
`Cargo.toml:79-82` — `profile.dist` uses `lto = "thin"` only. Set
`lto = "fat"`, `codegen-units = 1`, `strip = true` to shrink the dist binary.
Also document the ~80 MB one-time model download in the README/install script
so users aren't surprised on first run.

### Package patent in nixpkgs
Distinct from the Nix flake (already merged). Getting `patent` into nixpkgs
upstream so `nix profile install nixpkgs#patent` works. Needs a nixpkgs package
definition + upstream PR.

---

## New source adapters (tracking)

See issue #25 for the full adapter tracking list. High-value candidates for
0.6.0 scope:

- **Packagist** (PHP/Composer) — issue #24; simple JSON search API, large ecosystem
- **Reddit** — issue #21; `.json` read interface, no API key, high signal for real
  tool discussions
- **pub.dev** (Dart/Flutter) — fast-growing, has a search API
- **Hex** (Elixir/Erlang) — issue #41
- **Hackage** (Haskell) — issue #40
- **Artifact Hub** (CNCF cloud-native) — issue #42
- **JetBrains Marketplace** — natural companion to the VS Code adapter
