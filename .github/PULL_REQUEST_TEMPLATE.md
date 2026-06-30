<!-- Thanks for contributing to patent! Please fill in the checklist below. -->

## What this changes

<!-- A short description of the change and why. Link any related issue: "Closes #NN". -->

## Checklist

- [ ] `cargo fmt --all --check` passes
- [ ] `cargo clippy --all-targets -- -D warnings` passes
- [ ] `cargo test` passes
- [ ] Docs / README / CHANGELOG updated if behavior or flags changed

## If this adds a new source adapter

- [ ] Added the `Source` variant in `src/model.rs`
- [ ] Implemented `SourceAdapter` (with a `with_base_url` test constructor)
- [ ] Registered in `build_source()` **and** `detect_sources()` in `src/sources/mod.rs`
- [ ] Added a `wiremock` integration test in `tests/sources.rs` (happy path, empty results, server error)
- [ ] `every_built_source_is_reachable_from_some_idea` still passes

## If this touches the verdict / prompt / LLM backends

- [ ] Preserves the verdict-integrity rules — no copy that asserts something *doesn't* exist; output stays scoped to "found in the sources checked"
- [ ] The `guard_headline` / `floor_level` guards in `verdict.rs` are intact
