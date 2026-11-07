# 0001. SOTA design, tiered runtime

- Status: accepted
- Date: 2026-10-29
- Deciders: the owner

## Context

The grill handoff said to keep things lazy: every component must earn its place. On 2026-10-29 the owner asked for an architecture that is "SOTA and GOAT from day one, out-of-the-box, even if it seems we won't use over-the-top infra for a long time." Most of the handoff's leanings (NATS, Temporal, ClickHouse, Valkey, gVisor/Kata, a message broker) depend on which of those rules wins.

## Options

1. SOTA design with a tiered runtime: lean default profile, full-scale profile designed and shipped.
2. SOTA everything, all on by default.
3. Lean, as the handoff says: the second tier is added only when a trigger fires.

## Decision

Option 1. **The design is best-in-class from line one; the runtime is tiered.**

- **Best-in-class from the first line:**
  - the protocols and schemas (protobuf/Connect, session events, runner protocol)
  - the open specs (receipt, `rangelight.yaml`)
  - the CRDs
  - the signing and attestation (in-toto/SLSA)
  - the security model and trust boundaries
  - OTel
  - supply-chain hygiene
  - multi-tenancy seams
- **Two runtime profiles, both shipped by the one Helm chart:**
  - `lean`: the default, and the k3d quickstart.
  - `scale`: production HA.
- **Heavy infra is a real implementation, not a TODO.** Each piece sits behind an interface designed now and ships in the `scale` profile (or as an opt-in), never as a speculative stub.
- **Every ADR records both tiers:** what `lean` uses, what `scale` uses, and the interface between them.

## Consequences

- The quickstart stays small enough for a laptop, which matters for community adoption.
- Each heavy component costs us an interface plus a second implementation. That cost is accepted.
- CI must test both profiles.

## Revisit if

- Maintaining two implementations of a seam costs more than it returns. Then collapse to the `scale` one.
- The `lean` profile becomes a toy nobody runs in production.

## Amendment (external review, 2026-11-01)

**One implementation per seam; scale is configuration** (owner, after the external review).

- The SOTA *design* stays: contracts, the security model, signing, OTel.
- "lean" vs "scale" becomes Helm values (replicas, HA settings), not parallel implementations.
- These stay **documented options, unbuilt until a user asks:**
  - SPIFFE/SPIRE
  - private Rekor and self-hosted Sigstore
  - Sentinel/cluster Valkey
  - a 3-node NATS cluster
- **CI** tests one profile, plus an HA smoke test later.
