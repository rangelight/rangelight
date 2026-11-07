# 0007. Session kinds (plan | build) and warm = pre-provisioned, zero-pod

- Status: accepted
- Date: 2026-10-29
- Deciders: the owner

## Context

- The handoff started the story at "ticket → ready". The owner wants it to start earlier: a PM or engineer opens Rangelight with an idea, and a repo-aware agent helps design it and split it into an epic and tickets.
- The handoff's "warm namespace pool" read as always-on capacity, which seemed to conflict with scale-to-zero.

## Decision

**`Session.kind` is `plan` or `build`.** Both kinds share one runtime, one event log and one receipt chain.

- **`plan` sessions:**
  - read-only worktrees at `main` from the mirror, with no git write path
  - no previews by default; a preview of `main` is on request
  - typed outputs: a design doc, an epic, and tickets with draft contracts, sizes and predicted repos and services, written to the tracker
  - each ticket links back to the plan session's context digest, so the `build` session starts from it and Grill is shorter
- **`build` sessions:** as already designed (worktrees, previews, contract, receipt).
- **Both kinds are in M1.**

**Namespace and session states:**

| State | Exists | Running pods |
| --- | --- | --- |
| hot | everything | yes |
| warm | namespace, RBAC, NetworkPolicy, quota; images pre-pulled on nodes; caches primed; Argo params staged | **0** |
| cold | nothing | 0 |

- The warm pool holds **zero-pod** namespaces. Its size is a knob: `lean` defaults to 0–2, and 0 is allowed.
- Hibernating a session moves it from hot to warm: pods gone, the session rebuildable (ADR 0010). Waking it moves it back to hot.

## Consequences

- Scale-to-zero holds everywhere.
- A warm namespace costs disk only.
- Planning output becomes part of the provenance chain: from the PM's intent, through the contract, to the receipt.
- The planning layer the mockup spec left out is now in product scope (M1, lean).

## Revisit if

- Plan sessions need write access, for example to spike code. In that case, run them as a short-lived `build` session.
