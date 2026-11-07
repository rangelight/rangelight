# 0025. The POC is a contracts-first walking skeleton, public from day one

- Status: accepted. Revised on 2026-10-30: it was a throwaway spike.
- Date: 2026-10-30
- Deciders: the owner

## Decision

**Contracts first.** These eight extension points are written and versioned as `v1alpha1`, with written compatibility rules, before any implementation:

| # | Contract | Between | Format |
| --- | --- | --- | --- |
| 1 | Runner protocol | control plane ↔ runner | protobuf over NATS (ADR 0021) |
| 2 | Session protocol | agent runner ↔ Rangelight | protobuf events and commands |
| 3 | `rangelight.yaml` | repo ↔ Rangelight | JSON Schema |
| 4 | Evidence contract | gate container → receipt | JSON Schema plus an output directory |
| 5 | Receipt predicate | Rangelight → the world | in-toto `rangelight.dev/receipt/v1` (ADR 0022) |
| 6 | `rangelight-config` schema | org ↔ Rangelight | JSON Schema plus CEL (ADR 0005) |
| 7 | Integration traits | in-tree code | Rust traits (ADR 0017) |
| 8 | Endpoints | Rangelight ↔ infra | config (caches, IdP, secrets, S3, OCI) |

**Then the thinnest implementation behind each contract**, along the "seconds" hot path on k3s with the OTel Demo:
1. warm namespace
2. prebuild pod
3. alternates worktree
4. one Argo Application per session
5. Service swap
6. a Claude edit visible in the storefront

Every readiness layer is timed, and the timings run in CI as a performance regression guard.

**Private until the MVP** (ADR 0026 amendment), then public with the ADR log, the open specs and the timed demo. The history is kept clean from the first commit.

**Nothing is throwaway.** Implementations are deliberately minimal and can be replaced behind the contracts.

## Amendment (external review, 2026-10-31)

- **Public contracts at launch are reduced to three:** the receipt predicate, `rangelight.yaml` and the runner protocol.
- **Internal, versioned but not promised to outsiders:** the session protocol, the evidence contract, the integration traits and the endpoints config. They become public when a second implementer exists.
