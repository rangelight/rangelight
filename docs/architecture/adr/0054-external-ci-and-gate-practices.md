# 0054. External CI as evidence (GitHub Actions or Jenkins), and gate practices from a production pipeline

- Status: accepted
- Date: 2026-11-06
- Deciders: the owner
- Amends: ADR 0045 §3 (the observed gate), ADR 0041 (provenance classes), the receipt predicate and `rangelight.yaml` drafts
- Evidence: a production Jenkins multibranch pipeline that the owner runs on Kubernetes. It has a GitHub App for checks, path-based lanes, a tested-tree skip, credential-scoped fetch steps and per-gate Check Runs. Only the practices are recorded here.

## Context

- Rangelight's observed gate re-runs a repo's tests itself (ADR 0045). It never replaces an org's CI. Real orgs already have one, and it often does more than the tests: image builds, deploys, browser suites, fuzzing.
- Until now the receipt said nothing about that CI. A lane could finish "green" while the org's own required pipeline was red.
- Most orgs that aren't on GitHub Actions run **Jenkins**, usually on Kubernetes with the GitHub Checks plugin and a GitHub App. Both publish **GitHub Check Runs**, so one mechanism covers both, and any other CI that publishes Check Runs too.

## Decision

### 1. External checks are evidence, recorded with their own provenance

- **Declaring them:** `rangelight.yaml` declares the external checks a lane's head must pass, in `spec.externalChecks`. Each entry has:
  - `name`: the Check Run name, for example `Jenkins` or `ci / test`
  - `app`: the slug of the GitHub App allowed to publish it, for example `github-actions` or the org's Jenkins App
  - `required` (default true)
- **Reading them:** after `worker_done` and Rangelight's own gate, the `api` reads the Check Runs on the pushed head with the `rangelight-agents` App (Checks: read).
  - **A Check Run from any other app is ignored.** Without this rule, anyone who can create a check on the repo could forge one with the right name.
  - The lane is `ready` only when Rangelight's checks and every required external check succeed.
  - A failed external check goes back to the agent with its title, summary and URL, as a message at the next turn boundary (ADR 0045).
- **Recording them:** the receipt has a new `external[]` field: name, app slug, head SHA, conclusion, URL and the time it was read.
  - These have provenance **`reported`**, a third class alongside `observed` and `claimed` (ADR 0041). Rangelight saw the report, not the run.
  - The verifier and the Workbench never present `reported` as `observed`.
- **CI support:** GitHub Actions is the default and is tested first. **Jenkins is the second supported CI.** A docs page, `docs/ci/jenkins.md`, covers:
  - the GitHub App and Checks plugin setup
  - choosing the check name
  - the `app` slug to put in `rangelight.yaml`
  - polling, when webhooks can't reach Jenkins
  - The contract suite has recorded Check Run fixtures from both CIs, including a same-name check from the wrong app.

### 2. A receipt carries over when the tree is identical

- The receipt binds the **tree** SHA as well as the commit.
- After a merge, if the merge commit's tree equals the verified head's tree and the receipt's checks were green, the post-merge verification reuses the receipt. It records `reusedFrom: <head commit>` instead of re-running.
- Otherwise it re-checks, as ADR 0045 §3 already requires when `main` moves.
- This is the only shortcut. It is exact, because an identical tree builds identically under the pinned prebuild.

### 3. Credentials stay out of the gate pod

- The gate Job still holds no credentials (ADR 0045).
- Private dependencies (private git crates, private npm scopes, private Maven repositories) are fetched **through the package proxy** (ADR 0049, Plan 6), which holds the org's read-only credential.
- The gate pod sees only the proxy, and gets no other network egress (NetworkPolicy).
- **The pattern we are not copying:** handing a token to a "fetch" step inside the build. The branch under test controls that step (`.cargo/config.toml` can set a `rustc-wrapper`, and `build.rs` runs during fetch-adjacent steps), so the token is reachable by any branch author.

### 4. Checks the paths need ("gate lanes")

- `rangelight.yaml` may map path globs to named check sets (`spec.gateLanes`).
- **The rules:**
  - The diff is against the merge-base, with `--no-renames`, so a rename counts its old path.
  - **An unmapped path selects the full set.**
  - A change to `rangelight.yaml`, the gate lane map or any CI file selects the full set.
  - The map is read from the approved base branch, never from the branch under test (ADR 0041). The production pipeline we studied reads it from the tree it judges and has to compensate with a review rule.
  - A stacked PR (one whose base isn't the default branch) runs the full set.
- **The receipt** records which lane set ran and the paths that chose it, so a reviewer sees what was skipped.
- **When:** Plan 3 ships the full set only. Gate lanes come with Plan 7, once gate cost is measured.

### 5. Gate scheduling and test services

- **Slots and priority:**
  - Gate Jobs take a per-repo gate slot (the hot-slot model, Plan 7).
  - Verifications of `main` go first; lane verifications wait behind them.
  - A new push to a lane **cancels** that lane's running gate. A `main` verification is never cancelled; later merges fold into the one queued run.
- **Caches:** each slot keeps its own build cache with a size cap, removed before a run when over the cap. A shared cache across worktrees runs another worktree's test binaries.
- **Timeouts:** each step times only its own work, inside its slot. Waiting for a slot never consumes a test's timeout.
  - A timeout is reported as `timed_out`, never as a test failure.
- **Test services:** `rangelight.yaml` may declare the services a repo's tests need (`spec.tests.services`: Postgres, NATS, an object store and so on) as sidecar containers in the gate pod.
  - Their data lives on memory `emptyDir`s.
  - They get CPU **requests** sized above the test runner's and no CPU limits, so under load the stores are served first.
  - Build and test parallelism is bounded by environment variables, not limits.
  - **This is not the toolchain-sidecar design ADR 0013 rejected.** These are services in the gate pod, which runs no agent; the agent pod still has one container.
  - Ships with Plan 3.

### 6. Reports are scanned before they leave the gate

- Test reports, Check Run summaries and the events bundle pass a **secret and privacy scan** before they are published:
  - gitleaks rules, plus the org's patterns from `rangelight-config`
- On a hit, the report is dropped and the gate fails with `report-scan`, naming the rule, never the match.
- Ships with Plan 3.

### 7. One Check Run per check, results by test name

- Each of Rangelight's checks publishes its own Check Run. The summary lists what failed by name, and JUnit results are attached by test name, so a red run says what broke without opening a log.
- The `Rangelight receipt` Check Run (ADR 0048) summarises them all.

## Consequences

- An org's existing CI gates the lane, so Rangelight is additive, not a migration. That matters for design partners on Jenkins.
- The receipt gains a third provenance class; the verifier and the docs must explain it.
- The `rangelight-agents` App needs Checks: read on target repos. It already has Checks: write for its own runs.
- Gate lanes and tree reuse make the gate cheaper, at the cost of a map that must stay correct. Unmapped paths select the full set, so a wrong map can only make a run more expensive, never less safe.

## Revisit if

- A design partner's CI publishes commit statuses but not Check Runs. Then read the commit statuses API as well, still bound to one app or user.
- GitLab or Bitbucket arrive (ARCHITECTURE.md §9 "Target"). Their pipeline APIs map onto the same `external[]` field.
