# 0036. Engineering conventions: a previous project's, plus GitHub Actions and "why" comments

- Status: **superseded by ADR 0049** (2026-11-06). Its CI pipeline and the "why" comments rule carry over into ADR 0049 §9 and §10.
- Date: 2026-10-31
- Deciders: the owner

## Decision

**Adopted from a previous project's conventions:**
- the agent crew roles (architect, backend-rust, backend-go, dba, devops, security, frontend, uiux, …) in `.claude/agents/`
- Linear tickets, with the architect owning the backlog
- Ponytail: shortest diff, stdlib first, one runnable check per non-trivial piece of logic
- SQL check suites
- plain-English communication (TL;DR first)
- one Dockerfile per service
- sqlx

**Two changes for a project that will go public:**
1. **CI on GitHub Actions.** The pipeline runs:
   - Buf breaking-change detection
   - the schema diff
   - the SQL checks
   - a fuzzing smoke test
   - the receipt-verify fixtures
   - the hot-path timing check on k3d
   - cosign signing
   - SLSA provenance from the standard generator
2. **Short "why" comments are allowed in security-critical code** (gateway, crypto, RLS and policy), because outside reviewers won't have the tickets. Everywhere else, the no-comment rule applies.

**Exception to the "Rust only" rule:** Go is allowed under `runner/`, and TypeScript under `agent/` and `web/` (ADR 0018).
