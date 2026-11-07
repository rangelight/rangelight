# 0051. The name is Rangelight; the repo is public from commit 1

- Status: accepted
- Date: 2026-11-06
- Deciders: the owner
- Amends: ADR 0022 and ADR 0027 (the predicate type URI), ADR 0026 (public from commit 1), ADR 0042 item 6 (the name freeze), ADR 0050 (GitHub plan)

## Context

The working name, and two shortlisted alternatives, failed the checks on 2026-11-06: the GitHub org names are taken and the `.dev` and `.com` domains are registered. Of about 40 nautical names checked, four had a free GitHub org and a free `.dev`. Rangelight was the strongest.

Range lights are a pair of shore lights. A pilot keeps the front and rear light in line to stay in the channel into harbour. Rangelight lands a change only when two independent signals line up: the agent's claim and Rangelight's own check (ADR 0045 §3).

## Decision

### 1. Names

| Thing | Value |
|---|---|
| Product | Rangelight |
| GitHub org and repo | `github.com/rangelight/rangelight` |
| Domain | `rangelight.dev` (`.com` is registered by someone else) |
| Receipt predicate type | `https://rangelight.dev/receipt/v1alpha1` |
| JSON Schema `$id` base | `https://rangelight.dev/schemas/` |
| Kubernetes API group, label prefix | `rangelight.dev` (`Session.rangelight.dev`, `rangelight.dev/session`) |
| `apiVersion` of the repo file | `rangelight.dev/v1alpha1` |
| Repo file | `rangelight.yaml` |
| Images | `ghcr.io/rangelight/<service>` |
| Helm chart, system namespace | `rangelight`, `rangelight-system` |
| Env prefix, secret mounts | `RANGELIGHT_<SERVICE>_`, `/var/run/rangelight/secrets/` |
| Git refs | `refs/rangelight/sessions/…`, branches `rangelight/<lane-id>/<slug>` |
| Agent GitHub App, Check Run | `rangelight-agents`, `Rangelight receipt` |
| Linear label, MCP server name | `rangelight` |

These are constants, one per language (ADR 0049). They don't change after the first public commit. A future rename would be a new major API version, not an edit.

### 2. The repo is public from commit 1

The only reason to start private (ADR 0026, 2026-10-30 amendment) was the unsettled name. With the name settled:
- `rangelight/rangelight` is public from its first commit.
- The org is on GitHub's **Free plan**: rulesets, hosted runners and GHCR packages are free for public repos, so ADR 0050's Team-plan requirement is dropped.
- The images are public packages, so clusters pull them without a pull secret.
- The ADR 0026 scrub still applies to everything that moves over. ADR 0050's Context table stays in the design archive.

### 3. What keeps the working name

- **The design archive** (private) keeps its name.
- **The live POC** (`poc/` and the mockup's live view, `src/live/`) keeps the working name's label, branch prefix and paths. It's throwaway and isn't moved over (ADR 0026).
- **Historical records** (the external reviews, the handoffs, the 2026-10-30 shortlist in the decisions log, ADR 0026's original amendment) keep the name they were written with.

## Consequences

- **The owner, before Plan 1:** a USPTO and EUIPO search for "rangelight" in Classes 9 and 42 (a small Chicago company, Range Light LLC, uses the two-word name), then create the GitHub org `rangelight` and buy `rangelight.dev` (about $12 a year at Cloudflare).
- **If the trademark search fails,** this ADR is superseded before the first public commit. Afterwards, a rename is expensive (the predicate type is in signed receipts).
- **Plan 1** uses these values directly. It has no placeholder and no rename step.
- The docs in the design archive were renamed on 2026-11-06. Files were renamed too: `spec-drafts/rangelight-yaml.md`, `rangelight.yaml.v1alpha1.schema.json`, `otel-demo.rangelight.yaml`, `diagrams/rangelight-architecture.html`.

## Amendment (2026-11-06, groundwork research)

- **Trademark registration** (USPTO and EUIPO, Classes 9 and 42) and a published trademark usage policy on rangelight.dev are a **hard prerequisite for the hosted tier**, not only the search. The trademark, not the licence, is what stops a third party selling "Rangelight Cloud" (ADR 0052).
- **CNCF Sandbox requires transferring the trademark to the CNCF.** That decision waits until the hosted tier's economics are known. CLOMonitor's checks are still followed now, as a free checklist.

## Revisit if

- The trademark search finds a conflicting mark in Classes 9 or 42.
- `rangelight.com` becomes available at a reasonable price. Buy it as a redirect; the spec domain stays `rangelight.dev`.

## Amendment (2026-11-06, ticket key)

- **The Linear team key is `RAN`**, not `RL`: tickets are `RAN-<n>` and `TODO(RAN-123): …`. The short code prefix `rl` (`rl-*` crates, `rld`, `rl.` subjects, `rl_` roles) is unchanged.
- Branch names in Rangelight's own repos follow ADR 0050's 2026-11-06 branch amendment. `rangelight/<lane-id>/<slug>` stays the product's default lane branch for other orgs.
