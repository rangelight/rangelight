# 0014. Inner loop: Service selector swap

- Status: accepted
- Date: 2026-10-30
- Deciders: the owner

## Decision

- **Where a service under change runs:** as a hot-reloading process inside the agent pod, using the devcontainer toolchain (ADR 0013). Examples: air, nodemon, dotnet watch.
- **How it gets traffic:**
  - The operator parks the Argo-rendered Deployment for that service at 0 replicas, through the ApplicationSet parameter flip so self-heal agrees (ADR 0012).
  - It re-points the service's Kubernetes `Service` selector at the agent pod.
  - Neighbours call the service by its usual DNS name.
- **Config:** the operator projects the parked Deployment's env and ConfigMaps into the agent pod.
  - Only **preview-scoped** credentials are allowed: for example the session's own throwaway database user.
  - Never org secrets (ADR 0004).
- **Ending a change:** removing a service from the "under change" set reverses the swap.

## Consequences

- Pure Kubernetes, with no extra dependency.
- Changes take effect at hot-reload speed.
- The runtime is less prod-faithful than the service's own image. Gates (QA) can run against a built image when the contract requires it.

## Revisit if

- Services depend on sidecars or a mesh identity that a process in the agent pod can't reproduce. mirrord-style traffic stealing is the fallback.

## Amendment (external review, 2026-10-31)

- **The swap isn't done out-of-band.** Rangelight writes the per-session Argo Application (ADR 0019), so the swap is expressed **in that Application**: a Helm value or Kustomize patch that sets the service under change to 0 replicas and points its Service selector at the agent pod.
- **`ignoreDifferences`** is added only for anything that still can't be expressed declaratively.
- The result: self-heal never fights the swap.

## Amendment (2026-11-02)

- **This moves to v0.2.0, the public alpha** (ADR 0044, item 1). v0.1.0 has no Argo: the service under change runs inside the workspace pod (hot reload) and is reached through a port-forward.
- The decision itself is unchanged.
