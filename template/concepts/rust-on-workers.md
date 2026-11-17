---
title: Rust on Cloudflare Workers
summary: "how Rust runs on Workers: workers-rs + worker-build over wasm-bindgen, or a raw Wasm ABI driven from a JS Worker"
type: concept
updated: 2026-11-17
sources:
  - sources/2026-11-14-cloudflare-workers-rust.md
edges:
  - "EVIDENCED_BY 2026-11-14-cloudflare-workers-rust"
---

# Rust on Cloudflare Workers

Two ways to put Rust on a Worker. Pick by who owns the entrypoint.

## 1. All-Rust Worker — `workers-rs`
- Scaffold: `cargo generate cloudflare/workers-rs`. Needs
  `rustup target add wasm32-unknown-unknown`, `cargo-generate`, npm.
  `npx wrangler dev` (:8787), `npx wrangler deploy`.
- Entrypoint: `#[event(fetch)] async fn main(req: Request, env: Env, ctx: Context) -> Result<Response>`.
  Other events: `scheduled` (cron), `queue` (feature `queue`), `start` (runs once, e.g. a panic hook).
- `Env` gives typed bindings: `KvStore`, `Bucket` (R2), `D1Database`,
  `ObjectNamespace` (Durable Objects), `Queue`, `Fetcher`, `Ai`, `Hyperdrive`,
  `SecretStore`, `RateLimiter`, `Secret`/`Var`.
- `Context`: `waitUntil`, `passThroughOnException`. `Router` for multi-path. Docs: docs.rs/worker.
- **Async without tokio:** the handler runs under `wasm-bindgen-futures::spawn_local`
  as one JS Promise. One isolate, one event loop — the [[rust-style-guide]]
  rule against blocking an async worker applies doubly.
- `worker-build` writes the JS shim, bundles, runs `wasm-opt`. Release profile:
  `lto = true`, `strip = true`, `codegen-units = 1`. Size is a real limit —
  bundle cap and cold start grow with it.

## 2. Wasm library under a JS Worker
- **Workers hands you a `WebAssembly.Module`, not an `Instance`.** Plain
  `wasm-pack --target bundler` output assumes an instance: patch it to
  `new WebAssembly.Instance(mod, imports)` then `__wbg_set_wasm`.
- Cheaper for a pure compute kernel: **no wasm-bindgen, a raw ABI.** A thin
  `workers.js` takes the precompiled Module; a shared `core.js` instantiates it,
  reused by browser and Node hosts.

## Which one
- Touches bindings (KV/R2/D1/DO/Queues) → `workers-rs`.
- Portable kernel that also runs in browser/Node/wasmtime → raw ABI + thin JS host per runtime.

Back to the [[library]].
