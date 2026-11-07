# 0005. Org/team configuration: GitOps source of truth, shown live in the UI

- Status: accepted
- Date: 2026-10-29
- Deciders: the owner

## Context

Teams define their own SDLC flows, which covers:
- stages and gates
- agents, models and budgets
- escalation triggers, such as "after N QA⇄Sec loops, page a human"
- connectors
- templates

Security sets floors that no team can lower. We need one source of truth that the DevOps audience trusts, that receipts can cite, and that the UI shows clearly.

## Options

1. A GitOps config repo.
2. The database plus the UI.
3. Kubernetes CRDs.
4. A hybrid: git for pipelines, the database for operational knobs.

## Decision

Option 1. The owner added a condition: the configuration must be fully represented in the UI, for clarity, and it must hot-reload.

- **Source of truth:** one `rangelight-config` repo, laid out as `org/`, `teams/<team>/`, `policies/`, `skills/` and `connectors/`.
  - CODEOWNERS gives Security the floors and gives each team its own folder.
- **Layering:** rangelight presets → org template → team pipeline → repo `.rangelight/` (narrow overrides only), all merged with `extends:`.
  - Policy floors are CEL rules evaluated against the merged result.
  - JSON Schema and `rangelight config lint` validate it.
- **Sync:**
  - The control plane watches the repo through the in-cluster git mirror, reacting to the push webhook within seconds.
  - It compiles each commit into an immutable, versioned **config snapshot** in Postgres, keyed by commit SHA. Every value carries its provenance (which layer and file set it).
  - An invalid commit is rejected. The last good snapshot stays active, and the UI shows the error.
- **UI:**
  - The UI shows the *effective* config per team, with per-value provenance ("max_loops 3 · team checkout@a1b2c3 · floor 5") and a diff between snapshots.
  - Editing in the UI opens a PR against the config repo. A dry-run shows which teams and tickets the change would affect.
- **Hot reload:** a new snapshot applies immediately to everything that hasn't locked a contract:
  - the Up next queue
  - Grill
  - new sessions
  - dashboards
- **Running tickets:** the effective config is frozen into the contract when it locks, and the receipt cites the snapshot SHA.
  - A running session that is affected gets a Decision card: "Config changed: adopt v42?"
  - A **policy-floor tightening** (Security) applies to running sessions immediately and is recorded in the receipt.
- **Connectors:**
  - Each connector is one protobuf `TrackerConnector` service, hosted either in-tree (compiled in) or out-of-process (a container that serves the same proto).
  - Every connector goes through the same manifest and approval flow as an MCP.
  - Status and field mappings are per team, and live in that team's config folder.

## Consequences

- Config gets review, history, blame and rollback for free, through git.
- Receipts can reproduce the exact rules a ticket ran under.
- The UI is a projection plus a PR writer. It never mutates config directly.
- Non-git users (PMs, managers) edit through the UI and never see YAML.

## Revisit if

- UI-originated PRs become friction for fast operational knobs, such as on-call routing during an incident. In that case, consider allowing direct-commit for allowlisted paths.

## Amendment (external review, 2026-10-31)

- **Connectors:** the out-of-process protobuf `TrackerConnector` is superseded by ADR 0017 (in-tree only).
- **Linting:** `rangelight config lint` is replaced by `check-jsonschema` against the published schemas (ADR 0033).
- **Defer:** the "config changed, adopt v42?" Decision card and the dry-run come after launch.
