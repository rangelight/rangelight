# 0042. Decisions from the builder check

- Status: accepted
- Date: 2026-11-01
- Deciders: the owner for items 1–4; my calls for items 5–8.

## Decision

1. **Contracts at launch ("Grill-lite"):**
   - The agent drafts testable criteria from the ticket or the plan session, together with the gate that proves each one, and a budget.
   - The engineer edits them and **locks** them in a contract card in the Workbench.
   - Changes after locking are Decision cards.
   - Criteria are proven only by `observed` gate Jobs (ADR 0041).
2. **`rangelight-config` at launch is the core pipeline** (ADR 0005):
   - the repo is watched through the mirror and compiled into versioned snapshots
   - CEL floor checks run on every snapshot
   - receipts cite the snapshot SHA
   - unlocked work hot-reloads
   - the UI shows a **read-only** effective-config view with per-value provenance and errors
   - **Later:** editing from the UI (as PRs) and the dry-run.
3. **Quotas at launch:**
   - hot-slot caps per engineer and team (from config)
   - a visible queue with each session's position
   - P0 sessions jump the queue
   - admission logic lives inside the `api`
   - **Next rung:** env-hours, preemption by hibernation, the warm pool and a separate `admission` service.
4. **Launch = public alpha, with the wedge fixed:**
   - the timed OTel Demo on k3d, plus a tested install on one real cluster type (RKE2 or EKS)
   - labelled alpha, with no upgrade guarantees yet
   - HA values, the Barman restore drill and the full platform matrix come in the beta
   - **Fixed scope:** the copy of prod, the signed receipt, gates, and the threat-model fixes
   - **If the date runs well past 17 weeks:** the extras are cut last-in, first-out: Valkey, plan sessions, column grants, Merkle/WORM
5. **POC fallbacks (my call):**
   - RustFS Object Lock fails: checkpoints stay signed, the "WORM" claim waits, and we evaluate a BYO S3 with Object Lock.
   - Rootless BuildKit fails under Pod Security: BuildKit gets an isolated, privileged build namespace that only does trusted builds.
   - OIDC token validation fails on k3s: the runner relays a TokenReview.
6. **Name freeze (my call):** a plan milestone that must happen before the "go public" step.
7. **Contract-draft details (my call):**
   - **Parking a service under change:** the per-session Application's `park.values` removes the workload, and Rangelight serves a Service with the same name that selects the agent pod.
   - **Session-scoped NATS credentials** are minted by the control plane, not by the runner.
   - **The verifier works offline:** the receipt ships an **events bundle** (the session's events, as an OCI artifact referenced by digest) so the chain can be verified without API access.
   - **Cost:** one cost line per credential, summed in the receipt.
8. **Estimate (my call):** ADRs 0038–0042 add about 3–5 weeks, giving **about 15–22 weeks** for the public alpha at a previous project's pace. The cut order in item 4 protects the date.

## Amendment (second external review, 2026-11-02)

- Item 4's "fixed scope / cut order" wording is replaced by ADR 0044's v0.1.0 and v0.2.0 definitions. The four extras are ship-unless-slip for v0.2.0, with the go/no-go before v0.2 hardening.
- Item 8's estimate is kept only as a sanity check. Progress is measured by the adoption gates (ADR 0044, item 4).

## Amendment (2026-11-06)

- Item 6 is done: the name is **Rangelight** (ADR 0051), frozen before the first commit of the public repo rather than before "go public", because the repo is now public from commit 1.
