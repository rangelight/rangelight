# 0029. Event log: a hash chain per session plus signed per-org Merkle checkpoints

- Status: accepted
- Date: 2026-10-31
- Deciders: the owner

## Decision

- **One append-only `events` table in Postgres** is the source of truth (ADR 0021). Everything else is a projection of it: the feed, the receipt, the ledger, audit and analytics.
- **Each event carries:**
  - a UUIDv7 `id`
  - `org_id`, `team_id`, `session_id?`
  - a per-session `seq`
  - an `actor` (user, agent, runner or system)
  - a `type` and a versioned protobuf payload
  - a canonical encoding (deterministic protobuf)
  - `prev_hash` and `hash`
- **A hash chain per session.** Its head is written into that session's receipt (ADR 0022), so a receipt verifies on its own.
- **Org-level audit events** (logins, policy changes, unseals, authorization denials, approvals) form one chained stream per org.
- **Merkle checkpoints, Certificate Transparency style:**
  - Every few minutes, per org, a Merkle root is computed over every session head and the org audit stream.
  - It is signed with the org key (ADR 0022).
  - It is written to object storage with **Object Lock (WORM)**, and in `scale` optionally to a private Rekor.
  - The verify script (ADR 0033) proves both inclusion and that nothing was deleted.
- **Large payloads** (logs, traces, videos, transcripts) live in object storage, referenced from events by digest.

## Revisit if

- Checkpoint frequency or the size of the Merkle tree becomes a write bottleneck at hosted scale.

## Amendment (external review, 2026-10-31)

- **Verifying Merkle inclusion** needs more than a jq script. It arrives with the checkpoints themselves (v0.5+), together with a verifier.
