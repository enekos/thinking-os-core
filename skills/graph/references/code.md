# The code — one Rust crate, three binaries

Everything that reads or writes the graph lives in `tools/` of the
**thinking-os-core** repo, not in the wiki. The wiki holds data; the core repo
holds the engine. No Python, no LLM calls.

```bash
thinking-os update                                    # normal users: latest published tools
cargo test --manifest-path tools/Cargo.toml           # contributors: unit + integration tests
bash tools/e2e-zaindu.sh                              # offline event chain, then a live GitHub leg
```

`./install.sh` installs the published release by default. When developing this
checkout, use `./install.sh --from-source` to build the local binaries instead;
that is the only installation path that needs Rust.

| Binary | Module | Job |
|---|---|---|
| `sare` | `src/sare/` | extract · fuse · serve · retype · viz — the pipeline |
| `zaindu` | `src/watch/` | poll GitHub, diff, write ledger + proposals |
| `ikusi` | `src/ikusi/` | the viewer on `127.0.0.1:4747` |

The library crate is `os`: `kg` (ontology, frontmatter, TSV, Node/Edge),
`sare`, `watch`, `ikusi`. One third-party dependency crossed the line for
`ikusi`: `sutegi`, a public git dependency pinned to a tag, core only
(`default-features = false`) — no ORM, no async runtime. The first build needs
network to fetch it. Published release users do not build this dependency.

Source toolchain: Rust ≥ 1.85 (edition 2024).
