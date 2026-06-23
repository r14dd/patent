# patent 0.5.0 — richer data + wider net

_Local notes only (untracked). Deeper match data, more registries,
and the robustness/test work that supports both._

---

## Features (deferred from 0.4.0)

### Config file + env-var overrides (requested: fekkksn)
`~/.config/patent/config.toml` via `toml` crate, `#[derive(Deserialize)]` with
`deny_unknown_fields`, all fields `Option<T>`. Lives in `src/bin/patent/`.
Precedence: flag > env > config file > default. Env vars: `PATENT_MODEL`,
`PATENT_API_BASE`, `PATENT_API_KEY` (falls back to `OPENAI_API_KEY`). Source
filtering stays CLI-only. Missing file = silent; malformed file = hard error
with location.

### `--sources` / `--exclude`
Comma-separated single flag (`--sources crates-io,npm`). Mutually exclusive:
`--sources` sets the exact list, `--exclude` removes from auto-detected. Error
if both passed. Requires `FromStr`/`Display` on `Source` using kebab-case names.

### `--print-prompt` / "bring your own agent" mode (requested: walkernico)
Emit the verdict prompt to stdout after search+rank, then exit. Overrides
`--fast`, `--json`, and TUI. No file path argument — use shell redirection.

### Install script (issue #4 — emre-tiryaki)
Minimal: check for `cargo` (point at rustup.rs if missing), install C++ build
tools via distro package manager (apt, dnf/yum, pacman, apk, xcode-select),
warn on glibc < 2.38, then `cargo install patent`. Clean fallback message for
unsupported platforms. Plus a Homebrew formula as a separate distribution channel.

## Features (new for 0.5.0)

### Maintenance / abandonware signal (requested: toadi)
Add optional `last_updated` / `last_release` to `Match`, populate per adapter
where the API exposes it (crates.io `updated_at`, npm `time.modified`, GitHub
`pushed_at`…), show "last released X ago" in the TUI detail view + JSON, flag
stale matches, and blend recency + popularity into ranking. Touches the `Match`
model, all adapters + their wiremock tests, ranking, and display — large scope.

### Low-star repos don't surface from GitHub search
Reported: `0xBroom/lsport` (4 stars, Python CLI for TCP port inspection) doesn't
show up for "cli tool that kills a process on a port." Root cause: GitHub's
search API buries low-star repos; they never make it into the 20 results we
fetch. The crates.io `lsport` (a *different* project — SSH port TUI) does appear
but ranks low (sim 0.336) because its description doesn't match. Possible fixes:
request more results from GitHub (paginate or raise `per_page`), issue multiple
query variants, or add a secondary GitHub code/topics search. Overlaps with
broader search-recall work in 0.3.0 (T1-8/T1-9) but this specific long-tail
problem likely needs its own pass.

### Broader coverage (issue #1 — tomasriveral)
GitLab / Homebrew-as-search-source / AUR / Nixpkgs / Repology, or a
meta-aggregator (libraries.io). Each adds upkeep. AUR and Nixpkgs called out as
high-value (large community, many standalone tools not on the major registries).
Homebrew search adapter requested by masoko (search formulae+casks via the
`formulae.brew.sh` JSON API).

Additional registries not yet covered:

_Language ecosystems:_ **Packagist** (PHP/Composer, large ecosystem, has a
search API), **pub.dev** (Dart/Flutter, growing fast), **Hex**
(Elixir/Erlang), **Hackage** (Haskell), **Swift Package Index**, **CPAN**
(Perl).

_IDE plugins:_ **JetBrains Marketplace** — natural companion to the existing
VS Code Marketplace adapter.

_Windows package managers:_ **Winget** (Microsoft's official package manager,
growing catalog of CLI tools), **Chocolatey** / **Scoop** (large existing
catalogs, popular with Windows devs who are currently underserved).

_Linux desktop:_ **Snapcraft** (Ubuntu snaps), **Flathub** (Flatpak — has a
JSON API).

_Meta/discovery:_ **alternativeto.net** — explicitly built around finding
alternatives to existing software, high signal for prior art. **awesome-\*
lists** — search GitHub for `awesome-*` repos matching the query topic.

**Reddit** — `https://www.reddit.com/search.json?q=QUERY&sort=relevance&limit=25`
returns public post data with no API key required (appending `.json` to any
Reddit URL is an undocumented but stable read-only interface). The global search
already spans all subreddits, covering the full dev-tool space: r/programming,
r/devops, r/commandline, r/rust, r/Python, r/golang, r/javascript, r/vim,
r/neovim, r/linux, r/selfhosted, r/sysadmin, r/webdev, r/docker, r/kubernetes,
r/vscode, r/bash, r/cpp, r/node, r/SideProject, r/coolgithubprojects, and more.
High signal for prior art — real discussions where developers mention, compare,
and recommend tools. Requires a descriptive `User-Agent` to avoid rate limiting;
otherwise the same pattern as the existing HN adapter.

### Keyword-only / no-ML ranking mode (requested: MarsupialLeast145)
A flag that ranks by keyword overlap instead of embeddings, for users who refuse
any ML. `--fast` still loads the embedding model; this would be the truly
zero-ML path. Needs a separate ranking implementation.

### Generalise to a "tool finder" (suggested: Aloster)
Same machinery answers "what tool solves this problem." Mostly a verdict/copy
reframe + maybe a `--find` mode. Directional, lower priority.

---

## Robustness

### Library `rank()` blocks tokio if called from async
`rank.rs:136` — the convenience wrapper calls `Ranker::new()` + `embed_query()`
+ `rank_with()` synchronously. The binary wraps them in `spawn_blocking`, but
a library consumer in async context would block the runtime. Either document the
footgun or add an async `rank_async()`.

### `thiserror` v2 vs ecosystem
`thiserror = "2"` works but most of the ecosystem is on v1. Not a bug, just
unusual. Monitor.

### `expect()` on reqwest client builders panics in library code
`ollama.rs:28`, `openai.rs:24`, `sources/mod.rs:50` —
`reqwest::Client::builder().build().expect(...)` panics if TLS init fails. In
a library crate these constructors should return `Result`. Requires changing
`Ollama::new` and `OpenAi::new` signatures from `Self` to `Result<Self>`.

### No per-source wall-clock timeout
`mod.rs:278-289` — the retry block has no outer deadline beyond the 10s HTTP
client timeout. A source that connects but trickles data for 9.9s twice holds
up the entire `join_all` for ~21s. Wrapping the per-source retry block in
`tokio::time::timeout` would provide a stronger guarantee.

### `OpenAi` missing `Debug` and `Clone`
`openai.rs:11` — `Ollama` derives both; `OpenAi` derives neither. A manual
`Debug` impl that redacts `api_key` would be the ideal middle ground.

---

## Test gaps

### Malformed body tests for non-crates.io sources
Only crates.io has a test for a 200 response with a non-JSON body (e.g., CDN
returning HTML). The remaining 10 sources lack this. A real-world failure mode
that could cause confusing errors instead of graceful degradation. Add at least
for GitHub, npm, and Hacker News.

### Empty results tests for 6 sources
Only crates.io, GitHub, npm, PyPI, and Hacker News test empty-results paths.
Go, Maven, NuGet, RubyGems, Docker Hub, and VS Code Marketplace have no
coverage for this case.

### Source retry success test
The retry logic (`mod.rs:281-286`) is tested for "failure is tolerated" but
never for "transient failure recovers on retry." Need a wiremock test that
returns 500 once then 200, and verify the results come through.

### Timeout behavior test
The HTTP client has a 10s timeout (`mod.rs:42`) but no test verifies a
hanging server eventually errors. A `ResponseTemplate::new(200).set_delay(30s)`
wiremock test would catch this.

### No MSRV CI job
`Cargo.toml` declares `rust-version = "1.80"` but CI only tests stable. A dep
bump could silently break MSRV. Add a matrix entry with
`dtolnay/rust-toolchain@1.80` running at least `cargo check`.

---

## Distribution

### Package patent in nixpkgs (trtl_playz, r/linux)
Distinct from PR #5 (flake). Getting patent into nixpkgs so
`nix profile install nixpkgs#patent` works. Needs a nixpkgs package definition +
upstream PR.

### Windows prebuilt binary (issue #12 — sriharshaguthikonda)
`cargo install patent` fails on Windows due to ONNX Runtime / MSVC linker
errors (`libort_sys` unresolved externals). Add `x86_64-pc-windows-msvc` to
cargo-dist targets. May need to bundle the ONNX runtime DLLs or enable `ort`'s
`download-binaries` feature to get linking working on the GitHub Actions Windows
runner.

### Build fails on Ubuntu 22.04 (glibc too old)
walkernico (r/rust). The onnxruntime prebuilt needs glibc >= 2.38, Ubuntu 22.04
ships 2.35. Fix options: `ort` load-dynamic, build onnx from source, or
document the glibc floor. The prebuilt binaries + install script sidestep it for
users who don't build from source.

---

## Repo hygiene (keep local)

### Comment-style pass (optional)
Soften any older AI-tell-y comments already on GitHub if going further than
new-code-only cleanliness.
