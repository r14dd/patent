# Next

State carried over from the old Mac on 2026-11-12.

## In flight
- Inactive. `main` is at b163805 (v0.14.3 plus dependency bumps), CI and the scheduled live-source run are green, no open PRs or issues.
- `CHANGELOG.md` `[Unreleased]` has one entry, the MSRV raise to 1.92 (needed by `ort` 2.0.0-rc.15 via `fastembed` 7.6.1). It is not tagged.
- The `wip/pipeline-timeout` branch is an old WIP from the 0.7.1 era; `main` already has a per-source timeout (`SOURCE_TIMEOUT`, test `hanging_source_times_out`) and `src/bin/patent/pipeline.rs`.

## Gotchas
- Before a push or release run `cargo fmt --all --check`, then `cargo clean -p patent && cargo clippy --all-targets -- -D warnings`, then `cargo test`. CI tracks `@stable` with `-D warnings`, and a cached local clippy hides new lints; `clean -p` matches a fresh runner.
- Probe a source API with curl before writing its adapter, and add a `tests/live.rs` entry in the same commit. Wiremock tests only prove your own fixture.
