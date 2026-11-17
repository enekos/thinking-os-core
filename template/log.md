# log

Append-only. One `## [YYYY-MM-DD] op | subject` header per operation.

```bash
grep "^## \[" log.md | tail -10            # last 10 ops
grep "^## \[" log.md | grep ingest         # ingest history
```

---

## [YYYY-MM-DD] bootstrap | wiki created from thinking-os-core template
