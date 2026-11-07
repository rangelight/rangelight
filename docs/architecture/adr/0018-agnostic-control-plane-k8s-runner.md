# 0018. A K8s-agnostic control plane and a Kubernetes runner; admission vs placement

- Status: accepted. The language split was confirmed on 2026-10-30: Rust control plane, Go runner, TypeScript agent runner and web app.
- Date: 2026-10-30
- Deciders: the owner
- Amends: ADR 0012 (the operator is now "the runner"), ADR 0008 (Kueue is dropped)

## Context

- Rangelight runs only inside Kubernetes. A full prod-shaped environment needs it.
- The owner does not want the control plane to know it lives in Kubernetes, or to be a Kubernetes client.

## Decision

- **The control plane imports no Kubernetes client.**
  - It talks to **runners** over the **runner protocol** (protobuf, dial-out from the runner, mTLS).
  - This is the same seam the hosted model already needs.
- **The runner** is the only Kubernetes-aware component. It is the per-cluster Go controller (formerly "the operator"). It makes **no product decisions**: it executes them and reports facts.
  - **Writes the session runtime directly**, in seconds:
    - the namespace lifecycle (warm pool, claim, hot/warm/cold)
    - the agent pod, NetworkPolicy, ResourceQuota, RuntimeClass and projected token
    - the Service selector swap (ADR 0014)
    - hibernate and wake
  - **Hands neighbours to Argo CD**, which renders the org's charts (the mechanism is in the next ADR).
  - **Reports** allocatable capacity, Pending pods, phases, events and progress steps.
  - **Runs delegated local timers** (idle autostop), so sessions behave safely if the link drops.
  - Keeps `Session`/`WarmPool` CRs as its local desired state and for `kubectl` legibility (ADR 0012 stands).
- **Scheduling happens at two levels:**
  - **Admission (control plane, no K8s):**
    - quotas, env-hours, priority, the queue
    - preemption victims and bursts
    - decided against the capacity the runner reports
  - **Placement (the Kubernetes scheduler):**
    - PriorityClasses (system > P0 > normal) as a safety net
    - a Pending pod is reported back, and the control plane re-queues or preempts
    - node autoscaling (Karpenter or cluster-autoscaler) turns approved bursts into nodes
- **Kueue is dropped.** It has no notion of users, env-hours or tickets.
- **Proposed language split:**
  - the control plane in **Rust**: business logic, security, receipts
  - the runner in **Go**: controller-runtime, client-go, Argo types
  - The protobuf runner protocol is the contract between them.

## Consequences

- The control plane can run anywhere: in the cluster, hosted, or on a laptop against a remote runner.
- Two backend toolchains and two supply-chain pipelines, if the split is confirmed.
- Capacity estimates and Pending feedback must be accurate and fast.

## Revisit if

- Admission without Kubernetes knowledge misjudges capacity often. Then the runner gets a local admission veto.

## Amendment (external review, 2026-10-31)

- **Projected ServiceAccount tokens** are validated against the cluster's service-account issuer through OIDC discovery and JWKS over HTTPS. That's a plain HTTP client, not a Kubernetes client.
  - The issuer URL is part of each runner's registration.
  - k3s, RKE2 and EKS all expose issuer discovery. Verify this in the POC.

## Amendment (external review, 2026-11-01)

- **The runner stays in Go** (owner, 2026-11-01), despite the review's toolchain-cost argument.
- **Adopting `kubernetes-sigs/agent-sandbox`** (Sandbox, SandboxClaim, SandboxWarmPool) for the agent-pod lifecycle is **pending more data**.

### agent-sandbox evaluation (2026-11-01, code read at `9f7999b`)

- **Maturity:** v1.0.9, but the API is `v1beta1`. There is no written CRD compatibility policy, and a precedent of forced migrations. It is Google-led (about 60% of recent commits), a SIG Apps subproject, Apache-2.0. No adopters are listed.
- **Core `Sandbox`:**
  - a singleton pod from a raw `PodSpec` (compatible with PSS restricted, a custom SA, projected tokens, RuntimeClass and multiple containers)
  - `operatingMode: Suspended` deletes the pod and keeps PVCs; resume uses the current template
  - ready, scheduled and finished conditions; failed-pod recreate; `shutdownTime`
  - no NetworkPolicy of its own
  - it fits "hibernate = delete pod, rebuild from git" and preemption-by-suspend
- **Extensions (`SandboxClaim`, `SandboxWarmPool`, `SandboxTemplate`, router, SDKs) are unusable for us:**
  - all references are resolved in the same namespace, and cross-namespace adoption is rejected
  - so a pool can't serve `rl-<session>` namespaces
  - a managed template forces its own NetworkPolicy and public DNS, which would break neighbours
- **Cost:** a cluster-wide pod/Service/PVC RBAC controller with no namespace scoping. It saves only a few hundred lines.
- **Decision (owner, 2026-11-01):** write our own pod lifecycle now. Run the 1–2 day agent-sandbox POC (`docs/architecture/reviews` or the POC plan) in parallel with the main POC. Adopt the core `Sandbox` only if it passes (resume p95 no worse than ours, clean dependency build).
