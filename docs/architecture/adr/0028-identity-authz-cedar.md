# 0028. Identity and authorization: OIDC, agent principals, Cedar in-process, RLS underneath

- Status: accepted
- Date: 2026-10-31
- Deciders: the owner. Using Cedar for agent capabilities too is my call.

## Decision

- **Authentication:**
  - OIDC only. Dex is the bundled default; Okta, Entra and Keycloak are bring-your-own (ADR 0009).
  - SCIM comes later.
- **Principals:**
  - users and service accounts
  - agents: `agent:<session>/<lane>`, always acting *on behalf of* a user (ADR 0004)
  - runners
- **Authorization uses Cedar** (the `cedar-policy` crate, Apache-2.0), evaluated in-process in the Rust control plane.
  - **Roles:** engineer, PM, reviewer, manager, security, devops admin, team admin.
  - **Attribute rules:** owner, team, delegate, data class, tier.
  - **Policies** live in `rangelight-config` (ADR 0005) and are reviewed through CODEOWNERS. The Cedar analyser checks that team policies can't weaken Security's floors.
  - **Every authorization decision is emitted as an audit event.**
  - **My call:** agent capability checks (MCP tool calls, egress additions, gate skips) are Cedar requests too, so one policy language covers people and agents. CEL stays only for validating config values (ADR 0005 floors such as `max_loops ≤ 5`).
- **Tenant isolation:** Postgres RLS on `org_id`/`team_id` remains underneath as defence in depth (ADR 0003).
- **Kubernetes RBAC:** no mapping. The control plane has no Kubernetes client (ADR 0018). The runner has its own narrowly scoped ServiceAccount.

## Revisit if

- Sharing hierarchies get deep (nested groups, cross-org sharing in hosted). Then consider ReBAC (OpenFGA or SpiceDB).

## Amendment (external review, 2026-10-31)

- **The "Cedar analyser checks floors" step is dropped.** Cedar's forbid-overrides-permit semantics already keep Security's `forbid` policies from being weakened by any team `permit`.

## Amendment (2026-11-06, every seat)

- **A `viewer` role** (ADR 0057): read-only, no lanes, no scratch terminals; free and unlimited. A product manager's preview sign-off is a separate narrow permission.
