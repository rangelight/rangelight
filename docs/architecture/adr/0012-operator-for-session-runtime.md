# 0012. An operator owns the session runtime; Argo CD renders the neighbours

- Status: accepted
- Date: 2026-10-30
- Deciders: the owner

## Options

1. An operator for the session runtime, with Argo CD rendering the neighbouring services.
2. Argo-only: a session chart, with Rangelight serving only parameter sets.
3. An operator that renders everything, including the neighbours.

## Decision

Option 1.

- **Postgres holds product truth:**
  - tickets and contracts
  - decisions
  - the ledger
  - receipts
  - the event log
  - users and teams
- **CRDs hold the desired runtime state:**
  - `Session`: kind, priority, hot/warm/cold, repos and checkpoints, profile
  - `WarmPool`

  More CRDs are added only when a reconciler needs them.
- **The control plane writes the CRs. The operator:**
  - reconciles the namespace, agent pod, NetworkPolicy, ResourceQuota and service account
  - performs hibernate and wake (ADR 0010)
  - executes admission and preemption decisions (ADR 0008)
  - writes rich `.status` back: phase, queue position, progress steps, and who preempted the session
- **The operator serves the ApplicationSet plugin generator.** Each hot session is one parameter set. Argo CD renders the org's existing charts for the neighbouring services into the session namespace.
- **Ownership is split cleanly:**
  - The operator owns the agent pod and the services under change.
  - Argo owns the neighbours at `main`.
  - Hibernation of Argo-owned workloads uses a parameter flip, not an out-of-band scale, so self-heal never fights us.

## Consequences

- `kubectl get sessions -A` and `describe` work, and the Workbench reads the same status.
- Two sources of truth need a clear rule: the product decides in Postgres, and the CR is a projection that the operator reports back on.
  - A lost CR is recreated from Postgres.
  - A CR with no Postgres row is garbage-collected.

## Revisit if

- Argo's ApplicationSet latency becomes a start-time bottleneck for neighbours. Options then: a webhook refresh, a shorter requeue, or the operator rendering hot-path neighbours.
