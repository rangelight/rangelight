# 0019. Neighbours: one Argo Application per session, with PR-generator-compatible parameters

- Status: accepted
- Date: 2026-10-30
- Deciders: the owner
- Amends: ADR 0012 (the ApplicationSet plugin generator is no longer the primary mechanism)

## Context

- The ApplicationSet plugin generator is polled every `requeueAfterSeconds` (default 1800s). Its docs show no webhook refresh.
- The Pull Request generator (Argo CD docs, "latest", read 2026-10-30):
  - supports GitHub, GitLab, Gitea, Bitbucket Server and Cloud, and Azure DevOps
  - yields `number, title, branch, branch_slug, target_branch(_slug), head_sha, head_short_sha(_7), labels, author`
  - offers `branchMatch`, `titleMatch` and label filters
  - is refreshed by webhook on PR events (GitHub, GitLab)
  - creates an Application when a PR matches, and deletes it when the PR stops matching
- It doesn't fit Rangelight sessions:
  - sessions exist **before** any PR (plan, build, "local only · PR not opened")
  - it polls the SCM API, while Rangelight touches upstream only on purpose (ADR 0011)

## Decision

- **The runner writes one Argo `Application` per hot session**, targeting namespace `rl-<session>`.
  - It points at the org's charts or Kustomize with the preview overlay, from the in-cluster mirror.
  - It is created at claim and deleted at teardown.
  - Hibernation and the service swap are parameter changes on that Application.
  - A new Application syncs immediately, with no generator polling.
- **What we adopt from the PR generator:**
  - **The parameter contract.** The Application is templated with the PR generator's parameter names (`branch, branch_slug, head_sha, head_short_sha, number?, labels, author, target_branch`), plus Rangelight extras (`session_id, namespace, under_change, profile`).
    - An org's existing PR-preview ApplicationSet template can be reused as a Rangelight preview template with no changes.
  - **The slug rule:** RFC 1123, truncated to 50 characters.
  - **The lifecycle semantics:** exists while the criteria hold, deleted when they don't.
- **Coexistence with an existing PR generator:**
  - Rangelight labels its PRs (`rangelight`).
  - The docs recommend `excludedLabels: [rangelight]` on the org's own PR-preview ApplicationSet, so a Rangelight PR doesn't get a duplicate preview.
- **An ApplicationSet plugin generator mode** stays an *optional* integration for orgs that insist on ApplicationSets, with the latency trade-off documented.

## Consequences

- Seconds-level neighbour start, fully Argo-native, visible in the Argo UI.
- The runner needs RBAC to create Argo `Application` objects in the Argo namespace, scoped to an AppProject that admits only `lf-*` destinations.

## Revisit if

- Argo adds webhook or on-demand refresh for plugin generators. Then the generator becomes the primary mechanism again.

## Amendment (external review, 2026-10-31)

- **The per-session Application must use `syncPolicy.automated`** for the "syncs immediately" claim to hold.
- **`excludedLabels`** isn't supported for Bitbucket. The fallback is a branch prefix (`rangelight/`) plus `branchMatch`.

## Amendment (2026-11-02)

- **This moves to v0.2.0, the public alpha** (ADR 0044, item 1). v0.1.0 has no Argo: the service under change runs inside the workspace pod (hot reload) and is reached through a port-forward.
- The decision itself is unchanged.
