# Working on thinking-os-core

Engine only: `tools/` is the Rust graph pipeline (`sare`), watcher (`zaindu`)
and viewer (`ikusi`). `artalde/` is a separate Cargo workspace for the agent UI.
`skills/` holds linked agent workflows; `template/` is the new-wiki skeleton;
`install.sh` builds, links skills and upgrades missing template files.

## Find the right entry point

- CLI: `tools/src/bin/`; graph: `tools/src/sare/`; watcher: `tools/src/watch/`.
- UI model/probes: `artalde/crates/artalde-core/`; UI: `artalde/crates/artalde/`.
- Skill change: read its `SKILL.md` and only the relevant references.
- Wiki UI configuration: read `template/artalde/AGENTS.md`.
- Rust: read `template/concepts/rust-style-guide.md`; public types/API also
  `rust-type-driven-design.md`; concurrency also
  `rust-resource-and-concurrency-patterns.md`; tests/bug fixes also
  `verification-patterns.md`. Load other library pages only when needed.

## Commands

`make help` lists targets. Two independent Rust workspaces: check the one you
changed. `make check-tools` runs tools lint and tests; `make check-artalde`
runs UI formatting, lint and tests; `make check` runs both. `make test-tools`
is the quick offline suite. `make e2e` includes live GitHub reads.

## Bound the work

- Start with status and targeted searches; do not read whole generated graphs.
- Use `sare hop <node> -d 2` and `sare why <node>` for wiki context.
- One owner per changed file. Parallelize only independent work when authorized.
- Deterministic collection belongs in commands; skills own judgment and writing.
- GitHub Issues and Linear are independent work sources. Keep source ids
  unambiguous and preserve cached state on incomplete remote reads.
- Keep transcripts, diffs and tool logs out of durable graph memory.
- Never hand-edit generated TSVs/views. Change source, then rebuild.
- Run targeted checks while iterating; run required workspace checks before
  finishing. Report actual results and missing checks; do not claim speedups
  without measurements. Preserve unrelated edits; no AI-attribution trailers.
