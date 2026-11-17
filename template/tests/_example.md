---
title: Test result at one commit
summary: "one line describing the outcome, including the failing case on failure"
type: test
status: passed
ran_at: YYYY-MM-DDTHH:MM:SSZ
task: example-task
commit: "0123456789abcdef0123456789abcdef01234567"
command: "cargo test --manifest-path tools/Cargo.toml"
exit_code: 0
---

# Test result

- **Target:** the exact clean commit named in `commit`.
- **Outcome:** what passed, failed, or made the run inconclusive.
- **Evidence:** an optional short report path or failure identifier, not a transcript dump.
