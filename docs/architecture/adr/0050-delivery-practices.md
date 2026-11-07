# 0050. Delivery practices for building Rangelight: trunk, enforced gates, agent identity, tickets with acceptance tests

- Status: accepted
- Date: 2026-11-06
- Deciders: the owner
- Complements: ADR 0049 (how the code is written). This ADR is how work flows from ticket to running software.
- Evidence: an audit (2026-11-06) of the owner's previous project, a solo owner working with agent roles: three repos and ~1,000 tickets. The failures measured there are summarised as: lessons from a previous solo-plus-agents project.


## Decision

### 1. One trunk; releases are tags; promotion is by digest

- **`main` is the only long-lived branch.** Short-lived feature branches, squash-merged. No `develop`, no release branches before v1.0.
- **Every merge to `main`** builds every changed image once, tagged by commit SHA, pushed to GHCR by digest.
- **The dev cluster runs `main`.** `just dev-deploy` (Plan 1) pins the digests CI just built; from Plan 7 the Helm chart takes digests only, never tags. No hand-written pin PRs.
- **Releases** are signed tags (`v0.1.0-alpha.N`) cut from `main` with a generated changelog (Conventional Commits). A release is the same digests already running on dev.

### 2. The merge gate is enforced by GitHub, not by convention

- **Required:** branch protection or a ruleset on `main` with the required checks (`just check` parts), one approving review, linear history, signed commits, no bypass, including for the owner. Rulesets are free on a public repo (ADR 0051), so the Free plan is enough. **No merge while a required check is pending or red.**
- **CI time budget:** `just check` p90 ≤ 10 minutes. Path-filtered jobs per component, Rust caching (`Swatinem/rust-cache`, `sccache`), `cargo-nextest`. A PR that pushes p90 over budget gets a ticket the same week.
- **CI never shares a node with what it tests.** Checks run on GitHub-hosted runners; the dev cluster is a deploy target only. The org has a spending limit with an alert at 75%, so CI never silently stops.
- **Flaky tests are quarantined within a day** (a `#[ignore = "RAN-123 flaky"]` with a ticket), never rerun-until-green.

### 3. Agents have their own identity

- **A GitHub App for agent work** (`<name>-agents`): agent commits, PRs and reviews are under the App, not the owner. The owner's account is used only for what the owner did.
- **Agent reviews are recorded as GitHub reviews** by the App, with the reviewer role in the body. A human approval is still required for paths in CODEOWNERS (ADR 0049 §9).
- **In Linear,** agent comments and state changes go through an app actor (Linear's agent/OAuth app), so "who did what" is in the metadata, not the text.
- This is Rangelight's own receipt idea, applied to building Rangelight: from Plan 4 on, Rangelight's own lanes produce this work, and the `Rangelight receipt` check is required on its repo (dogfooding, ADR 0002).

### 4. The owner is not the only gate

- **CODEOWNERS covers the security-critical paths only** (ADR 0049 §9). Everything else merges on green checks plus one review, which may be the agent App's review plus the receipt.
- **Decisions the owner must make are batched**, once a day, in one Linear view (`needs-owner`), never inline in a ticket's comment thread. A ticket waiting on the owner for more than 2 working days is raised in the daily batch automatically.

### 5. Tickets are contracts

- **Every ticket has numbered acceptance criteria**, each one checkable by a test (`AC1`, `AC2`, …). The tests that prove them are named `AC<n>` (ADR 0045 §3), so the criteria are proven the same way Rangelight proves a customer's.
- **Acceptance changes are edits to the description with a dated "Revised" line**, never comments. A change after work starts moves the ticket back to Todo.
- **One ticket, one PR.** A change that spans components is still one PR, in the one repo (the monorepo makes this possible).
- **Templates:** `Feature`, `Bug` (with reproduction), `Spike` (time-boxed, output is a doc). The template carries the AC block.

### 6. Linear reflects reality, automatically

- **States move by automation, not by hand:** a branch or PR linked → In Progress; PR ready → In Review; merged → Done. Bulk state edits aren't allowed.
- **WIP limit:** at most 3 tickets In Progress per actor (owner or agent App) and 5 In Review in total. Over the limit, nothing new starts.
- **Every ticket is in a project**, and a project has a target gate, not only a date (ADR 0044 adoption gates).
- **Labels:** one small, documented set: `area/<component>`, `kind/bug|feature|chore|security`, `needs-owner`, `blocked`. No date labels; no synonym pairs.
- **Priority:** Urgent only with a running incident or a broken `main`; High is at most 20% of open tickets. The weekly review rebalances.
- **Scope:** a ticket created after a plan starts goes to the backlog, and joins the plan only by an explicit decision recorded on the plan's ticket.
- **Weekly cycles** with a 30-minute review: throughput, cycle time, oldest In Progress, flaky tests, CI p90.

### 7. Tests prove behaviour and never touch live data

- **Integration and journey tests exercise the real edge**, not its config: an auth test sends a real request through the real proxy; a policy test evaluates real Cedar (ADR 0049 §8).
- **Alerts are on in every environment that runs scheduled checks**, routed to the owner's phone for dev too. A red scheduled check opens a ticket automatically.
- **No test, agent or CI job ever has a credential for live data.** Test configs refuse non-test endpoints at startup (a `RANGELIGHT_ENV=test` guard that rejects any endpoint not on a test allowlist). This is ADR 0004's rule, applied to our own tooling.
- **`gitleaks`** runs in `just check` and on push; agent sessions never get secrets in their environment.

### 8. Fail closed everywhere

- A missing credential, a missing config value or a skipped deploy step **fails** the job. "Skipped" is never green.
- Infra code (Helm chart, `deploy/`, hooks) has CI like any other code: `helm lint`, `helm template | kubeconform`, `shellcheck`, `chart-testing` on k3d.

### 9. Measured, not assumed

The weekly review (§6) tracks five numbers, from GitHub and Linear's APIs by a small script (`scripts/metrics.sh`):

| Metric | Target |
|---|---|
| CI `just check` p90 | ≤ 10 min |
| PRs merged with a non-green required check | 0 |
| Median ticket cycle time (In Progress → Done) | tracked; trend down |
| Tickets waiting on the owner > 2 working days | 0 |
| Flaky tests not quarantined within a day | 0 |

## Consequences

- **Plan 1, Task 1** adds the GitHub App for agents, the ruleset, path-filtered CI with caching, `gitleaks`, `check-deploy`, the Linear templates and automations, and `scripts/metrics.sh`.
- **The owner provides** the GitHub org `rangelight` (Free plan; the repo is public from commit 1, ADR 0051) and a Linear team for Rangelight with the workflow above.
- This adds process on day one. It's small next to the measured cost of not having it.

## Revisit if

- The team grows beyond the owner. Then add a second human CODEOWNER for each security path, and two-person approval for the receipt signer and the gateway.

## Amendment (2026-11-06, groundwork research)

- **§2 and §4, GitHub never counts a self-approval.** "One approving review, no bypass, including the owner" therefore means:
  - an owner-authored PR is approved by the agent identity's review (§3), and an agent-authored PR by the owner;
  - **paths in CODEOWNERS** (security-critical) change only through agent-authored PRs that the owner approves, until a second human maintainer exists.
- **The agent identity is tested on day zero:** whether a GitHub App's approving review satisfies the ruleset's required review (and Scorecard's Code-Review check) isn't documented. If it does, the App is the agent identity. If it doesn't, the agent identity is a **machine user** (`rangelight-bot`, a separate GitHub account, as GitHub's terms allow), with 2FA, write access only, and its own SSH signing key; the App stays for Check Runs and webhooks.
- **Commit identity:**
  - Agent commits are authored and committed by the agent identity, carry a `Rangelight-Lane: <lane-id>` trailer when a lane made them, and the owner's `Signed-off-by` (DCO) as the accountable human.
  - Signed by the identity itself: the App's commits are made through the GitHub API (GitHub signs them), the machine user's with its own SSH key.
  - The owner's commits use the owner's GitHub noreply email and the owner's SSH signing key. A personal email never appears in public history.
- **§2 ruleset additions:** merge queue (or "require branches to be up to date"), last-push approval, dismiss stale approvals, and a tag ruleset on `v*` (only the release workflow creates tags).
- **§1 releases:** cosign signatures (`.sigstore.json`), SLSA provenance (`.intoto.jsonl`) and the SPDX SBOM are uploaded **as GitHub release assets**, not only to GHCR (Scorecard counts release assets only). Immutable releases are on.

## Amendment (2026-11-06, branch names)

- **Every branch except `main` is named `<type>/RAN-<n>-<word>-<word>`**: type `feat`, `bug`, `hotfix` or `release`, the Linear ticket, and a two-word lower-case description. For example `feat/RAN-42-receipt-verify`.
  - Pattern: `^(feat|bug|hotfix|release)/RAN-[1-9][0-9]*-[a-z0-9]+-[a-z0-9]+$`
  - `bug` is an ordinary fix through the normal flow. `hotfix` fixes a released version and is cut from its tag.
  - `release` is a short-lived release-prep branch merged into `main` by PR. This replaces "no release branches before v1.0" above: there are still no long-lived branches besides `main`.
  - Exempt: `main`, Renovate's `renovate/*`, and the merge queue's `gh-readonly-queue/*`.
- **Enforced by CI, not by a ruleset:** branch-name rules aren't available on the Free plan. A `branch-name` check, part of the required checks, fails a PR whose head branch doesn't match.
- **Agent lanes use the same names** in Rangelight's own repos. The type comes from the ticket's `kind/*` label; the lane ID goes in the `Rangelight-Lane:` commit trailer, not the branch.
  - The lane branch name is a per-repo setting (Plan 4); other orgs keep the default `rangelight/<lane-id>/<slug>`.
  - The mirror's `pre-receive` hook (ADR 0011) checks the **exact** branch name assigned to the lane, not a prefix, so a lane still can't push any other branch.

## Amendment (2026-11-06, Linear setup)

- **The `kind/*` labels add `kind/spike`**, the owner's choice on day zero, so the Spike template (§5) has a matching label. The set is `kind/feature`, `kind/bug`, `kind/spike`, `kind/chore`, `kind/security`.
- **The labels are flat names**, not Linear label groups, so tools read `kind/feature` as written.
- **Branch names use upper-case `RAN-`, and the `branch-name` check is case-sensitive.** Linear's "copy git branch name" gives lower case (`ran-1-…`), with no setting to change it, and Free rulesets can't restrict branch names. So a copied name is never used as-is: agents build the name from the ticket identifier, and the required `branch-name` check rejects `ran-…`. Its failure message prints the correct name. Linear still links `RAN-` branches, because its matching ignores case.

## Amendment (2026-11-07, architect session)

- **Correction to the groundwork amendment:** the owner signs commits with the GPG key `FC345EEA1DBBCC84` (noreply uid only), not an SSH key.
- **Every `just check` in this ADR reads `script/check.sh all`** (ADR 0049 amendment 2026-11-07: no task runner, Bash files in `script/`). The required checks and the `gate` job don't change.
