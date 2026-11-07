# 0027. Contract versioning: Kubernetes-style maturity, CI gates, a skew policy

- Status: accepted
- Date: 2026-10-30
- Deciders: the owner

## Decision

- **Each of the eight contracts (ADR 0025) carries its own maturity:**
  - `v1alpha1`: may break, with a changelog entry.
  - `v1beta1`: additive changes only.
  - `v1`: frozen. Changes need a new version and a deprecation window.
- **CI gates:**
  - `buf breaking` for the protos
  - a JSON Schema diff gate for the schemas
  - either one blocks an accidental break
- **Skew policy:** control plane N supports runner and agent-runner versions N-1 and N.
- **Receipts are verifiable forever.** Predicate versions are never removed, and the verifier keeps every version.
