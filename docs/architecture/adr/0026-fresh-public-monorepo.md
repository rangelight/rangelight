# 0026. A fresh public monorepo; this repo stays private as the design archive

- Status: accepted
- Date: 2026-10-30
- Deciders: the owner

## Decision

- **A new public monorepo with a clean history from commit 1:**

```
spec/           protos + JSON Schemas (Buf; generates Rust, Go, TS)   ← contracts 1–6, 8
control-plane/  Rust workspace (API, admission, event log, receipts, policy; no CLI, ADR 0033)
runner/         Go (the Kubernetes runner)
agent/          TypeScript (Agent SDK runner)
web/            TypeScript (Workbench, seeded from the mockup)
charts/         the Helm chart with optional dependency subcharts
docs/           PRODUCT.md, architecture/ (ADRs, decisions log, diagrams)
```

- **Repo hygiene from day one:**
  - Apache-2.0
  - DCO
  - CODEOWNERS
  - signed commits
  - a SLSA release pipeline
- **What moves over:** the ADRs, `PRODUCT.md`, the diagrams and the mockup, each scrubbed first.
- **What stays private:** the design archive: the handoff, references to previous projects and the mockup history.

## Amendment (2026-10-30): private until the MVP; name and domain pending

- **The repo starts private** in a new GitHub org and becomes public at the MVP. This supersedes "public from the first commit" here and in ADR 0025.
  - Because it will go public, the history must stay clean from commit 1.
- **The name is pending.**
  - A shortlist of alternatives is being checked.
  - The domain is bought after the MVP.
- **Spec URIs** (for example the receipt predicate type) use a placeholder namespace until the domain is secured, and are frozen before going public.

## Revisit if

- Release cadences diverge sharply between components (for example the runner vs the control plane). Then split repos along the contracts.

## Amendment (external review, 2026-10-31)

- **`control-plane/` has no CLI** (ADR 0033).

## Amendment (2026-11-06)

- **Layout details and the engineering standard are ADR 0049** (`services/`, `crates/`, `queries/`, `config/`, `justfile`).
- **Public from commit 1 again, as `rangelight/rangelight`** (ADR 0051). The 2026-10-30 "private until the MVP" amendment is superseded: it existed only because the name and domain weren't settled. The history-stays-clean rule is unchanged.

## Amendment (2026-11-06, groundwork research)

- **Repos:** `rangelight/rangelight` (the monorepo) and `rangelight/.github` (org profile, default community files, the reusable release workflow that gives SLSA Build L3). Nothing else public until a trigger below fires. No Homebrew tap (no CLI, ADR 0033), no proposals repo (the ADRs are the proposals), no chart repo (the chart ships to GHCR as OCI), no website repo (`docs/` builds the site).
- **Split trigger added:** `spec/` moves to its own repo only when an outside project consumes it.
- **Paid code never lives in the public repo** (ADR 0052): no `ee/` directory, no licence key, no feature gate. The hosted tier is a private `rangelight/cloud` repo that deploys the public images and chart by digest.
