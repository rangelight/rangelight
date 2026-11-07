# 0015. Agent pod sandbox: a hardened baseline plus a pluggable stronger runtime

- Status: accepted
- Date: 2026-10-30
- Deciders: the owner

## Decision

- **Baseline on every platform:**
  - Pod Security Standards set to `restricted`
  - non-root, all capabilities dropped
  - seccomp `RuntimeDefault`
  - read-only root filesystem, except the workspace and tmp
  - no automounted ServiceAccount token: only an audience-bound projected token (ADR 0004)
  - default-deny egress
  - Tetragon TracingPolicies recommended (they ship with the chart as optional)
- **Stronger isolation:** a `RuntimeClass` per profile (gVisor or Kata).
  - Policy can require it, for example: "untrusted repos and community extensions → gVisor".
- **Preflight:** a Helm pre-install hook Job reports which runtimes the cluster supports, and the receipt records the runtime each session used.

## Consequences

- Works on EKS managed nodes, RKE2, bare metal and k3s.
- Orgs that can run gVisor or Kata get VM-grade or user-space-kernel isolation by policy.
- The baseline must be tested in CI against real agent workloads (builds, browsers, language servers), so that hardening doesn't break them.

## Revisit if

- An escape-class incident, or a customer requirement, makes gVisor the default.

## Amendment (external review, 2026-10-31)

- **`rangelight preflight`** is a Helm pre-install hook Job (ADR 0033), not a CLI.
