# 0031. State and event in one transaction, with a transactional outbox to NATS

- Status: accepted
- Date: 2026-10-31
- Deciders: the owner

## Decision

- **Every command, in one Postgres transaction:**
  - appends its event(s) to the chained log (ADR 0029)
  - updates the current-state tables
  - inserts an outbox row
- **An outbox relay** publishes to NATS (ADR 0021). Consumers are idempotent by event ID.
- **Authority:**
  - State tables are authoritative for "now".
  - Events are authoritative for "how we got here".
  - Projections (feed, analytics, receipts) can be rebuilt from events.
- **A CI check** replays events and compares the result with the state tables.
- **Data access:** sqlx with compile-time-checked SQL (a convention proven on a previous project).
