# Contributing to patent

Thanks for considering a contribution! This guide covers the basics.

## Getting started

```bash
git clone https://github.com/r14dd/patent.git
cd patent
cargo build
```

**Build deps** (Linux only):
- Fedora / RHEL: `sudo dnf install openssl-devel gcc-c++`
- Ubuntu / Debian: `sudo apt install libssl-dev g++`

## CI gates

Every PR must pass these three checks (they run on GitHub Actions):

```bash
cargo fmt --all --check          # formatting
cargo clippy --all-targets -- -D warnings   # lint (warnings are errors)
cargo test                       # unit + wiremock integration tests
```

Run all three locally before pushing.

## Adding a new source adapter

patent has a well-worn pattern for adding search sources. Each source is a
separate module that implements the `SourceAdapter` trait. The checklist:

1. **Add the `Source` variant** in `src/model.rs` — the enum that identifies
   where a match came from.

2. **Implement `SourceAdapter`** in a new file under `src/sources/` (e.g.
   `src/sources/homebrew.rs`). Look at an existing adapter like `crates_io.rs`
   or `npm.rs` for the shape — constructor taking a `reqwest::Client`, a
   `with_base_url` alternate for testing, and the `search` method that returns
   `Vec<Match>`.

3. **Register it** in `src/sources/mod.rs`:
   - Add a `pub mod your_source;` declaration.
   - Add a branch in `build_source()` that constructs it.
   - Add detection rules in `detect_sources()` so it's selected for relevant
     queries.

4. **Add a wiremock integration test** in `tests/sources.rs`. The test should
   cover at least: a happy-path response with results, an empty-results
   response, and a server error (non-200).

5. **Verify the guard test passes**: `every_built_source_is_reachable_from_some_idea`
   will fail if a source can be built but no query ever selects it.

## Finding things to work on

Check the issue tracker for labels:
- [`good first issue`](https://github.com/r14dd/patent/labels/good%20first%20issue) — self-contained tasks with an existing pattern to follow
- [`help wanted`](https://github.com/r14dd/patent/labels/help%20wanted) — larger tasks where outside help is welcome
- [`new-source`](https://github.com/r14dd/patent/labels/new-source) — source adapter work

Comment on an issue before starting so we don't duplicate effort.

## Verdict integrity (non-negotiable)

This is the product's whole point — please don't soften it in a PR:

- The tool can prove something **exists**; it can **never** prove something
  *doesn't* — it only searched some sources. All verdict copy is scoped to
  *"found in the sources checked,"* never *"this doesn't exist."*
- The sources-checked list is always displayed, and selected sources that
  failed are surfaced as "not reached," so a thin result is never mistaken for
  "nothing out there."
- `verdict.rs` enforces this in code (`guard_headline` scrubs absence claims;
  `floor_level` floors the level against the similarity data). Changes to
  `verdict.rs`, the prompt, or `ollama.rs`/`openai.rs` must preserve these
  guards — a reviewer will hold the line on it.

## Code style

- `thiserror` for library errors (`patent::Error`), `anyhow` in the binary.
- Sources are best-effort and independent — one source failing never fails the
  whole run.
- Keep test data realistic (use actual registry response shapes, not stubs).

## License

By contributing, you agree that your contributions will be licensed under the
same MIT OR Apache-2.0 dual license as the project.
