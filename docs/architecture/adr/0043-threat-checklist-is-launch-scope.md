# 0043. The threat model's §7 checklist is launch scope

- Status: accepted (my call, within the owner's "public alpha, wedge fixed" decision; ADR 0042)
- Date: 2026-11-01

## Decision

Every item in `threat-model.md` §7 that is still open is a **launch requirement**. The implementation plan carries each one as a task with its test or CI check. This includes the design-level items:

- **Previews:**
  - Previews live on a **separate registrable domain** from the Workbench.
  - The Rangelight cookie is stripped before a request is proxied upstream.
- **Session start:** a session starts only on an **authorized human action** (a member clicks Start, or a member applies a label). Opening an issue never starts a session.
- **Sensitive files:** changes to CI workflows, CODEOWNERS, `rangelight.yaml` or devcontainer files raise a Decision card and are flagged in the receipt.
- **Runner write scope:** a ValidatingAdmissionPolicy confines the runner's writes to `lf-*` namespaces and allowlisted kinds.
- **Prebuild images:** session pods are admitted only with image digests the control plane pinned.
- **Git mirror:** `uploadpack.hideRefs` hides other sessions' refs.
- **NATS:** per-session JWTs with exact subject permissions, rate limits and size limits.
- **Masking:** an automated PII scan of the golden copy's output.

## Amendment (second external review, 2026-11-02)

- §7 items 19 (fail-closed masking and the PII scan) and 20 (golden-copy job credentials) move with `masked-snapshot` (ADR 0044, item 2). They're entry gates for that capability, not v0.1 or v0.2 items.
- §7 item 14 (RustFS Object Lock) follows Merkle/WORM, which is ship-unless-slip for v0.2.
- §7 items 1, 2, 17 and 18 (previews, the profile source, the AppProject) are v0.2.0 items, because Argo and previews arrive there (ADR 0044, item 1). Every other open item is v0.1.0 scope.
