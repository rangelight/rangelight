# 0021. NATS JetStream is the required transport, including the runner protocol; Postgres stays the source of truth

- Status: accepted
- Date: 2026-10-30
- Deciders: the owner
- Amends: ADR 0018 (the runner protocol is carried over NATS, not a custom gRPC tunnel)

## Context

- Session events flow up (to the feed, receipt, ledger and audit). Human input flows down (composer messages, Decisions, comments).
- In the hosted model, runners and agents sit behind customer NAT, and only outbound connections are allowed.
- A previous project already runs NATS JetStream in production (chart `nats` 2.14.6: WorkQueue streams, a dedup window, ack-wait, max-deliver, and consumer-lag alerts through the exporter).
- **Licence:** NATS stayed in the CNCF under Apache-2.0 after the 2025 Synadia dispute. CNCF holds the domain and the repos (cncf.io/blog/2025/05/01/protecting-nats-and-the-integrity-of-open-source-cncfs-commitment-to-the-community).

## Decision

- **NATS JetStream is required in every profile:**
  - `lean`: a single server.
  - `scale`: a 3-node cluster.
  - `hosted`: a **leaf node** in each customer cluster dials out over TLS to our hub.
- **Everything moves over NATS subjects, with protobuf payloads:**
  - the runner protocol (request/reply for runner control)
  - session events (up)
  - commands (down)
  - agent-to-agent typed messages (handoff H2)
- **One NATS account per org** for tenant isolation, with decentralised JWT/nkey credentials per runner. Agent pods get session-scoped credentials limited to their own subjects.
- **Subject ACLs enforce who may talk to whom.** For example, a lane may publish only to its session's subjects. A cross-session Agreement goes through control-plane subjects.
- **Postgres is the source of truth.**
  - The control plane consumes from JetStream and appends to the hash-chained event log idempotently, by event ID and per-session sequence.
  - The Workbench and the receipt read projections of Postgres.
  - NATS handles transport, buffering, fan-out and work distribution only.
- **Live fan-out** to control-plane replicas and browser sessions uses core NATS subjects.

## Consequences

- No custom tunnel or write-ahead log.
- Outages are buffered by JetStream on the leaf or cluster.
- One more required component, which is small: a single server on one node.
- Stream retention and replicas must be set by Rangelight (a lesson from a previous project: "stream replicas are set by the app").

## Revisit if

- NATS governance or licensing changes again.
- A customer forbids a broker in the data plane. The fallback is gRPC dial-out through the runner.

## Amendment (external review, 2026-10-31)

- **Who assigns `seq`:**
  - The agent publishes raw events on `ingest.session.<id>`, with a client event ID for idempotency.
  - The control plane's per-session consumer is the **single writer**. It assigns `seq`, computes the hash and appends, in one transaction with the state and the outbox (ADR 0031).
  - The outbox republishes the *committed* event on `events.session.<id>` for fan-out.
  - The agent never writes the chain directly.
- **NATS KV is not used for session state.** Postgres is the truth, and the CR is a cache (ADR 0012). Presence and leases can use NATS KV later, if needed.

## Amendment (2026-11-06)

- **A lane's NATS credentials live in the `rld` sidecar**, not the agent container, so the model can't read them (ADR 0045 §5).
