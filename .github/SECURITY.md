# Security Policy

## Supported versions

Only the latest released version of `patent` receives security fixes. Please
upgrade to the newest release before reporting an issue.

## Reporting a vulnerability

Please **do not** open a public issue for a security vulnerability.

Instead, use GitHub's private vulnerability reporting:
[**Report a vulnerability**](https://github.com/r14dd/patent/security/advisories/new).

If that is unavailable, contact the maintainer privately and we will arrange a
secure channel.

When reporting, please include:

- the version of `patent` (`patent --version`) and your OS,
- a description of the issue and its impact,
- steps to reproduce, and
- any relevant logs or output (with secrets redacted).

## Scope and what to consider

`patent` makes outbound HTTP requests to many third-party registries, talks to a
local LLM endpoint (Ollama at `localhost:11434`), and can send the search query
to a remote OpenAI-compatible API via `--api-base` using a key from `--api-key`
or `OPENAI_API_KEY`. Reports about credential handling (e.g. a key or query
leaking into logs or `--json` output), request handling for `--api-base`, or the
parsing of untrusted registry responses are especially welcome.

## Disclosure

We aim to acknowledge a report within a few days and to coordinate a fix and a
disclosure timeline with you. Please give us a reasonable window to release a fix
before any public disclosure.
