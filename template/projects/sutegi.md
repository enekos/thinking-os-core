---
title: sutegi
summary: "the owner's Rust web framework: std-only, zero third-party deps, ~480 KB binaries, agent-native (/__introspect, /__tools); default backend for new apps"
type: project
status: active
updated: 2026-11-17
---

# sutegi

**One-liner.** "Laravel for Rust": a batteries-included web framework built on
`std` only — no tokio, hyper or serde — with an AI agent as a first-class user.
Basque *sutegi* = forge. Public: github.com/enekos/sutegi · crates.io `sutegi`.
Latest release at template time: **v0.14.0**.

Clone it and add `path:` to this page's frontmatter once it is on this machine;
the git extractor then tracks its activity and PRs.

## What you get

- **Core** (always): HTTP/1.1 thread-pool server, router, `App` builder,
  middleware, typed extractors, SSE/streaming, JSON codec.
- **Opt-in features** on the facade, so the binary carries only what you use:
  `orm` · `derive` (`#[derive(Model, Validate)]`) · `validate` · `sqlite`
  (single node) · `postgres` (pure-std driver, multi-pod) · `queue` (durable,
  either backend) · `graceful` · `hexagon` (clean-architecture primitives) ·
  `session` · `auth` (users, guards, API tokens) · `template` · `mail` ·
  `auth-mail` · `storage` (fs, S3/R2, presigned) · `storage-db`. Also realtime
  (WS, channels, presence), actors + supervision, event sourcing, migrations with
  deterministic schema diff, `sutegi repl`.
- **Agent surface**: `GET /__introspect` (routes, models, tools, capabilities as
  stable JSON), `GET /__tools` (LLM tool manifest), `POST /__tools/<name>`.
  Closed by default: 403 without an `ops_guard`. `AGENTS.md` in the repo is the
  contract.
- **CLI**: `sutegi new <app>`, `sutegi make:model`, `sutegi make:route`,
  `sutegi introspect`.

## Using it

```toml
sutegi = { version = "0.14", default-features = false, features = ["sqlite"] }    # single node
sutegi = { version = "0.14", default-features = false, features = ["postgres"] }  # multi-pod
```

- Read `docs/TUTORIAL.md` (Todo app) and `docs/HEXAGONAL.md` in the repo first.
- Handlers take one `Ctx`, return anything `IntoResponse`; `serve()` reads
  `HOST`/`PORT`/`WORKERS` and drains on SIGTERM.
- Defaults fail closed: ops 403 without a guard, same-origin WebSockets, 415 on
  non-JSON bodies, `sslmode=require` errors. Session secrets must be ≥ 32 bytes.
- Terminate TLS at the load balancer.

## Gotchas

- Upgrading across a security release can invalidate every session cookie —
  read `CHANGELOG.md` before bumping.
- `cargo test` does not rebuild a binary target; rehearse upgrades with a fresh
  `cargo build` before trusting a migration check.
- Rust rules for working in it or on apps built with it → [[rust-style-guide]].

## Next up
- [ ] Clone it here and set `path:` if you'll build on or contribute to it.
