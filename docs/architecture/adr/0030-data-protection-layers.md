# 0030. Data protection: RLS for rows, column grants for data classes, encryption for sealed content

- Status: accepted
- Date: 2026-10-31
- Deciders: the owner

## Decision

- **Rows:**
  - RLS on every table.
  - Each transaction sets `app.org_id`, `app.team_id` and `app.principal` with `SET LOCAL`. This pattern is proven on a previous project.
- **Columns:**
  - Postgres column privileges per app role (`GRANT SELECT (…)`). The roles are a small fixed set: `rl_engineer`, `rl_manager`, `rl_security`, `rl_runner`, `rl_system`, and so on.
  - Each transaction runs as `SET LOCAL ROLE rl_<role>`, chosen by Cedar (ADR 0028).
  - Column classes: cost amounts, PII, transcript bodies, credential references.
- **Sealed content** (transcripts, prompt bodies, secret-adjacent text):
  - AES-GCM encryption under per-session data keys, wrapped by OpenBao Transit or the cloud KMS.
  - Unseal is an audited decrypt (handoff §3.9).
  - Retention and legal erasure work by **crypto-shredding** (destroying the key).
  - Hashes in the chain cover the ciphertext, so the chain stays verifiable after shredding (ADR 0029).
- **Proof:**
  - SQL check suites in CI (a pattern proven on a previous project) assert role × table access for both rows and columns.
- **The object store default is RustFS** (Apache-2.0, versioning and Object Lock listed as supported, checked 2026-10-31).
  - Object Lock compliance mode is verified in the POC.
  - This closes the open object-store item in ADR 0009.

## Revisit if

- Per-request role switching collides with connection pooling. In that case, use pgdog/pgbouncer in transaction mode, which a previous project already does.

## Amendment (external review, 2026-10-31)

- **RustFS 1.0** shipped on 2026-09-16 and is young. Object Lock and WORM compliance claims wait for proof in the POC.
