# 0003. M1 scope: trackers and tenancy

- Status: accepted
- Date: 2026-10-29
- Deciders: the owner

## Decision

- **Trackers in M1: Linear and GitHub Issues.**
  - Linear is the hero UX.
  - GitHub Issues gives a zero-signup quickstart on the OTel Demo fork (ADR 0002), reusing the GitHub App.
  - Two implementations keep the tracker seam honest.
  - Jira is the first connector after M1.
- **Tenancy in M1: one org, several teams, with RLS enforced.**
  - `org_id`/`team_id` on every row, and Postgres row-level security from the first migration.
  - Teams, team RBAC and team budgets are live.
  - Multi-org and the hosted control plane are designed but not shipped.

- **M1 slice:**
  1. A `plan` session turns an idea into tickets (ADR 0007).
  2. A `build` session starts.
  3. A namespace is claimed from the warm pool.
  4. Worktrees are created from the mirror.
  5. Argo previews the neighbouring services.
  6. Claude builds the change, in QA⇄Sec loops bounded by config.
  7. Playwright proves it.
  8. A PR opens with a signed in-toto receipt.
  9. The merge is verified on the prod Argo app.

  It runs on one cluster, with GitHub as the only forge, and the k3d quickstart on the `lean` profile.
- **Fork/rewind is in M1** (ADR 0010).
- **Out of M1, seams designed now:**
  - Fleet Wall
  - cross-session agent Agreements
  - the improvement loop and retro
  - Jira
  - A2A
  - Rangelight as an MCP server
  - multi-org and the hosted tier
  - Flux

- **MVP = M1 plus capacity basics** (decided 2026-10-31):
  - hot-slot and env-hour quotas per engineer and team
  - an admission queue with an ETA
  - P0 queue-jumping
  - preemption by hibernation, with auto-resume (ADR 0008)
  - **Not in the MVP:** admin-approved bursts, cross-team borrowing and autoscaler integration, all in M2.

## Consequences

- The quickstart needs no third-party signup beyond GitHub.
- Every query path runs under RLS from day one, so tests need a tenant context.
- The OTel Demo is split into demo teams (for example checkout, frontend and platform). This makes team-owned interfaces and cross-team Agreements demoable.

## Revisit if

- A design partner needs Jira in M1.
- The hosted tier is pulled into M1.
