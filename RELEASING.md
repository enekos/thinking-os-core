# Releasing thinking-os-core

Normal users install release bundles and never need Cargo. The release tag is
the distribution version; the Rust crates retain their own package versions.

## Publish a release

1. Ensure `main` is green in the quality workflow and review the changes to
   `skills/`, `template/`, and `deploy/` that will ship in the bundle.
2. Update `VERSION` using semantic versioning and commit that change with the
   release notes. Add migration notes when users must manually merge ontology
   or `CLAUDE.md` changes.
3. Push a tag whose name is `v$(cat VERSION)`:

   ```bash
   git tag v$(cat VERSION)
   git push origin v$(cat VERSION)
   ```

The `release` workflow checks the tag, runs the offline installer test, builds
locked release binaries on Ubuntu 22.04 and macOS 15, smoke-tests all four
tools, and uploads one complete archive per target. It then publishes the
archives, `SHA256SUMS`, and the standalone `install.sh` to GitHub Releases.

The supported targets are:

| Target | Archive contents |
|---|---|
| `x86_64-unknown-linux-gnu` | `sare`, `zaindu`, `ikusi`, `artalde`, skills, template, deploy files |
| `aarch64-apple-darwin` | The same complete bundle for Apple Silicon |

Run the workflow manually with **Run workflow** to produce downloadable build
artifacts without publishing a GitHub release. This is the required dry run
for changes to packaging or release automation.

## Verify a published release

On a clean supported machine, without Rust installed:

```bash
curl -fsSL "https://github.com/enekos/thinking-os-core/releases/download/v$(cat VERSION)/install.sh" | bash -s -- --no-timer
thinking-os version
thinking-os update --check
```

Also run `packaging/test-install.sh` locally. It uses fixture archives and fake
GitHub/tool commands; it does not contact GitHub or require Rust.

## Recovery

The installer downloads and verifies a new archive before activating it. If
the checksum or smoke test fails, the previous `current` symlink remains in
place. Do not delete the previous version directory until the new release has
been verified. Re-uploading assets for an existing tag should be limited to
fixing a failed release; never silently replace a release that users may have
installed.
