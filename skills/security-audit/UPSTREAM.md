# Upstream

Vendored from <https://github.com/cloudflare/security-audit-skill> (MIT, see `LICENSE`)
at `c1c8a8c1471069fb0e188eeaff69b8e8db6564a8`.

Local changes — keep this list exact when re-syncing:

- `LIVE-VALIDATION.md` — new: owner-authorized Phase 3b replay against a
  non-production deployment named by a wiki profile
  (`concepts/security-live-validation*.md`).
- `SKILL.md` — description mentions live validation; Universal execution
  safety gains the live-validation exception; Source ref says audit
  `origin/<default>` or the deployed tag; `execution_policy` gains the live
  value; workflow step 3b; anti-pattern 3 points at the companion.

Everything else, including both validators, their tests and
`report-schema.json`, is unchanged. Re-sync: copy upstream `skills/security-audit/`
over this directory, then re-apply the list above.
