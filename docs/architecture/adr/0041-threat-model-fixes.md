# 0041. Launch-blocking fixes from the threat model

- Status: accepted (2026-11-01). The owner accepted items 1–4 and decided items 5 and 6.
- Date: 2026-11-01
- Source: `docs/architecture/threat-model.md`

## Decision

1. **Evidence the agent can't forge.**
   - Gates (tests, Playwright, scans) that prove contract criteria run as a **separate gate Job** in the session namespace, which the runner starts.
   - The gate Job uses a read-only checkout of the **committed SHA**, not the agent's working process.
   - Every event carries a provenance class:
     - `observed`: written by the runner, the gateway or a gate
     - `claimed`: reported by the agent
   - The receipt marks a criterion **proven** only from `observed` evidence. Agent-run tests stay useful for the inner loop, but they are labelled `claimed`.
2. **Preview cookies.**
   - Preview auth cookies are **host-only** (no `Domain` attribute), one per preview host.
   - They are issued through a redirect to the `api`'s own domain, which hands out a short-lived, single-host token (the oauth2-proxy-per-host pattern).
   - A compromised preview page can't read the Workbench session or any other preview's cookie.
   - This amends ADR 0039.
3. **The preview setup can't be changed by the agent.**
   - `rangelight.yaml` is read from the **approved base branch** (or the config repo), never from the session branch.
   - The per-session Argo AppProject allows:
     - **sources:** only the in-cluster mirror
     - **destinations:** only `rl-<session>`
     - **resources:** namespaced kinds from an allowlist; no cluster-scoped kinds
   - Any change to `rangelight.yaml` itself is a normal PR, reviewed by humans.
   - This amends ADR 0019.
4. **Masking fails closed.**
   - Greenmask rules are an **allowlist**. A column that isn't listed is nulled.
   - A schema change the rules don't cover fails the golden-copy job, and the last good copy stays in use.
   - This amends ADR 0038.
5. **Package egress at launch:**
   - **git-pkgs/proxy** is the single read-only pull-through proxy for npm, PyPI, Go, crates, Maven, NuGet and more.
   - It doesn't proxy publish requests, so `npm publish` and its equivalents are impossible from a session.
   - Cooldown is on.
   - Its denylist is fed by our sync from `ossf/malicious-packages`.
   - **Zot** serves OCI images.
   - BYO Artifactory or Nexus endpoints are first-class.
   - It runs as its own container under GPL-3.0. A legal check on aggregation is needed before going public.
   - Package hosts are removed from the egress allowlist, which ends the temporary rule in the ADR 0016 amendment.
   - This closes the parked package-cache item.
6. **Keys:**
   - **OpenBao is part of the launch** (MPL-2.0). It provides Transit keys for:
     - receipt signing
     - checkpoint signing
     - wrapping the per-session data keys used for sealing
   - No key ever leaves OpenBao.
   - BYO Vault or a cloud KMS works through the same interface.
   - This overrides the ADR 0037 deferral of OpenBao to v0.4.

## Amendment (2026-11-06)

- **The observed gate Job runs Rangelight's four checks** (tests in its own run, main's tests on main merged with the branch, committed and pushed, merges cleanly), and criteria are proven only by `AC<n>` tests the branch added (ADR 0045 §3).

## Amendment (2026-11-06)

- **A third provenance class, `reported`:** an org's own CI result read from a GitHub Check Run published by the one app `rangelight.yaml` names for it (ADR 0054 §1). It is never shown as `observed`.
