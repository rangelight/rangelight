# 0056. Adoption features: verify without a cluster, public verifier and badge, claim-gap benchmark, push decisions, injection test set

- Status: accepted
- Date: 2026-11-06
- Deciders: the owner, on the CPO review of the roadmap
- Amends: the roadmap (new Plan 3a; additions to Plans 3, 5, 6 and 11), ARCHITECTURE.md §9 and §10, ADR 0052 §7 (what counts as telemetry)

## Context

- The roadmap builds the engine well: lanes, the observed gate, receipts. But the first step for a newcomer is installing a Kubernetes platform, which few take for a project they haven't tried.
- Receipts are invisible outside the cluster that made them.
- The claimed-criterion gap (ADR 0045) is a number nobody else measures.
- The owner was the bottleneck in the last project: 11 of 14 in-progress tickets waited on them (ADR 0050).

## Decision

### 1. `rangelight/verify`: a GitHub Action, no cluster needed (v0.1, new Plan 3a)

- **What it does:** it runs Rangelight's four checks and the `AC<n>` criteria gap on a PR, in the repo's own GitHub Actions. It reads `rangelight.yaml`'s `tests`, `externalChecks` and gate lanes. It publishes the `Rangelight receipt` Check Run and attaches a signed receipt.
- **Same code as Plan 3:** the gate logic and the receipt builder are reused, not copied. The Action is a thin entry point that ships with each release.
- **Signing:** keyless Sigstore with the GitHub Actions OIDC identity. The certificate names the repo and the workflow.
- **Being honest about trust:**
  - The receipt's checks have provenance `observed` with **`observer: ci`**, plus the workflow identity. A cluster gate has `observer: rangelight`. The verifier and the badge show which.
  - The workflow must come from the default branch. It is triggered by `pull_request` with read-only permissions; `pull_request_target` is never used. The Action refuses to sign when the workflow file differs from the default branch.
  - Fork PRs get the checks but no receipt (ADR 0048).
- **Its place:** the top of the adoption funnel. The README's first step is "add this workflow". Installing the platform is the second step, for lanes.

### 2. A public verifier page and a badge (v0.1, Plan 3 plus a site task)

- **The page:** a static page at `rangelight.dev/verify`.
  - Paste a receipt, a DSSE envelope or a PR URL. It verifies **in the browser** with the receipt verifier compiled to WebAssembly, against the embedded Sigstore trust root (ADR 0006).
  - Nothing is uploaded. A PR URL is fetched from GitHub's public API by the browser itself.
- **The badge:** "Verified by Rangelight". It links to that page with the receipt reference, for PR descriptions and READMEs. It shows the observer (`rangelight` or `ci`).
- **Counting:** the page may count verifications server-side as a bare counter, with no IP, repo or receipt content stored. This is the only usage number the project collects. It isn't telemetry under ADR 0052 §7: nothing in a cluster sends it; a person chose to visit the page.

### 3. The claim-gap benchmark (v0.2, Plan 11, with a spike in Plan 5)

- **The method:** a fixed, public set of real tickets on public repos, starting with the OTel Demo fork and the Now in Android fork.
  - Each ticket has locked `AC<n>` criteria.
  - The set is run through several coding agents (Claude Code first; others via verify-any-branch, ADR 0048).
- **What it publishes:**
  - the **claim gap**: criteria an agent claimed that Rangelight's gate couldn't prove
  - plus pass rate and cost per ticket
- **The rules:**
  - Runs are reproducible from the published set and receipts. Every number links to its receipts.
  - Re-run every release.
  - Vendors get the method in advance and no special treatment.
- **The spike in Plan 5:** the ticket-set format, and one manual run.

### 4. Decisions from the phone (v0.2, Plan 5)

- The Workbench is an installable web app (a manifest and a service worker).
- `needs-owner` and Decision cards send **web push** notifications, with one-tap answers for the card's options.
- A tap opens the card in the Workbench, behind the same sign-in, so a decision is always an authenticated action. No native app.

### 5. A public prompt-injection test set (Plan 6 builds it, Plan 11 publishes it)

- **What it is:** a versioned set of hostile inputs: repos, tickets, comments, file contents and MCP responses. Each one tries to make a lane:
  - leak or use a credential
  - push or write outside its branch
  - reach blocked egress
  - claim proof it doesn't have
  - start another session
- **How it runs:** in CI on every release against a real lane. Its pass rate is on the README.
- **Where it lives:** `test/injection/`, Apache-2.0, so other agent platforms can run it too.

### 6. The scorecard (no feature; discipline)

- **The headline metric:** **verified merges per week** (PRs merged with a green receipt). The secondary metric is the claim-gap rate.
- **Before going public:** OpenSSF Scorecard 9+, and the bestpractices.dev "passing" badge (Plan 11 gate).
- **The README** opens with a 60-second recording: ticket → lane → verified receipt → merge. It's recorded from the Workbench story mode, and re-recorded from the real product at v0.2.

### 7. Not added, on purpose

- more agent runtimes (verify-any-branch covers other agents)
- Jira and GitLab
- a native mobile app
- promoting a scratch terminal to a lane

Each waits for a user who asks.

## Consequences

- There are two observers of the same checks, and the receipt and the verifier must keep them apart.
- `rangelight.dev` hosts one dynamic counter. It needs a privacy note.
- The benchmark creates a public obligation to re-run it every release, and possible friction with vendors. The open method is the answer to both.
- Plan 3a adds work to v0.1. It's the cheapest plan to cut if v0.1 slips, but it's the adoption path, so it's cut last among the extras.

## Revisit if

- GitHub changes how Actions OIDC claims identify the workflow file. §1's trust rule depends on it.
- A vendor disputes the benchmark method with a reproducible counter-example.
