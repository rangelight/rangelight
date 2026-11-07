# 0032. Valkey from day one, scoped; Dragonfly as a bring-your-own option

- Status: accepted
- Date: 2026-10-31
- Deciders: the owner

## Context

- agentgateway's global (cross-replica) rate limits call an external rate-limit service. The Envoy reference server keeps its counters in Redis (agentgateway.dev docs, read 2026-10-31).
- **Licences:**
  - Valkey is BSD-3-Clause, under the Linux Foundation.
  - Dragonfly is BSL 1.1 until 2030-11-01.

## Decision

- **Valkey ships as a subchart from day one:**
  - `lean`: a single node.
  - `scale`: Sentinel or cluster mode.
- **Our Rust gateway uses Valkey counters directly** for global limits (ADR 0034). There is no Envoy ratelimit server.
- **Valkey's scope is limited to:**
  - rate-limit counters
  - hot caches (Cedar policy and authorization snapshots, ETags)
  - idempotency keys with TTL
- ~~**NATS KV holds** presence, leases, soft file locks and live session state.~~ Withdrawn by ADR 0044: no NATS KV at launch; presence and leases are Postgres rows with TTL columns if needed.
- **Postgres holds** anything durable: the quota ledger and env-hours.
- **Bring your own:** any Redis-protocol endpoint (Dragonfly, ElastiCache, Memorystore) via a URL. Dragonfly is not bundled, because of its licence.

## Revisit if

- A component needs Redis-specific features that Valkey drops, or Valkey's governance changes.
