# 0048. Verify any branch, wherever it came from; a remote MCP server for local-to-remote handoff

- Status: accepted
- Date: 2026-11-04
- Deciders: the owner
- Amends: ADR 0022 (receipt producer), ADR 0033 (its "revisit if" clause is triggered), ADR 0040 (GitHub App permissions), ADR 0044 (v0.1.0 scope)
- Numbering: 0045–0047 are reserved for the workspace lifecycle, credential modes and repo onboarding ADRs proposed on 2026-11-03.

## Context

Developers already run agents locally (Orca, Cmux, Claude Code, Cursor). They won't migrate to Rangelight for "cloud-native"; teams adopt it for trust in what lands. The live POC showed the receipt checks run on a pushed branch, not on the pod that produced it. So Rangelight can verify work from any source, and local tools can hand work to Rangelight instead of being replaced by it.

## Decision

### 1. Verify any branch (v0.1.0)

- **What runs:** the same checks as for a Rangelight lane:
  - the repo's tests in Rangelight's own run
  - the baseline run on `main`
  - the tests on `main` merged with the branch
  - committed and pushed (the verified SHA is the remote head)
  - merges cleanly into `main`
  - criteria proven only by tests the branch added
- **Triggers:**
  - a pull request opened or updated in a connected repo; per repo in `rangelight.yaml` (`verify.pullRequests`, default `true`)
  - the MCP tool `verify_branch` (§2)
  - **Verify a branch** in the Workbench
- **Ticket link:** from the branch name or the PR title and body (Linear's `TEAM-123` convention). With no linked ticket, the receipt has checks and no criteria.
- **Where it runs:** the observed gate Job (ADR 0041, threat-model §7 #12):
  - a hardened pod with no credentials and the normal egress policy
  - the code is treated as untrusted
  - no agent and no LLM tokens
- **Same-repo branches only.** Branches from forks are never verified in v0.1, so outsiders can't run code on our compute.
- **One check per branch:** a new push cancels the running check and verifies the new head. A per-repo concurrency limit (`verify.maxConcurrent`, default 4) queues the rest. Verification Jobs don't use the team's hot slots (they run no agent).
- **The receipt:** the same in-toto/DSSE format and signer (ADR 0022), with `producer: external`.
  - It carries only what Rangelight observed.
  - Agent-side fields (transcript, model, capabilities, event chain) are absent and marked "not recorded: produced outside Rangelight". They are never inferred.
- **Where it shows:**
  - a GitHub Check Run named `Rangelight receipt` with the checks, the criteria and a link to the receipt
  - a comment on the linked Linear ticket
  - a lane in the Workbench marked *external*
- **Check conclusion:**
  - `success` only when every check passes and every criterion is proven
  - otherwise `failure`, listing what failed or is only claimed
  - teams make the check required in branch protection
- **Rangelight doesn't merge external branches.** Merging stays on GitHub. Approve-merge in the Workbench stays for Rangelight lanes, and follows the same rule: no approval while a criterion is only claimed.
- **GitHub App permissions** (amends ADR 0040):
  - add **Checks: read and write**
  - add **Pull requests: read and write**
  - subscribe to `pull_request` events

### 2. A remote MCP server for local-to-remote handoff (v0.1.0)

- **Why MCP:** it's how local agents already take tools, and it meets ADR 0033's "revisit if" clause without building a CLI.
- **Shape:**
  - a remote endpoint served by the `api` at `/mcp` (Streamable HTTP)
  - a thin adapter over the same Connect services and Cedar checks
  - nothing to install: `claude mcp add --transport http rangelight https://<host>/mcp`
- **Auth:** OAuth 2.1 per the MCP authorization spec.
  - The `api` is the resource server and Dex is the authorization server.
  - Tokens are short-lived, bound to the `/mcp` audience, and act as the member, with Cedar applied.
  - No personal access tokens.
  - Client registration follows ADR 0044 item 7's order: Client ID Metadata Documents first, then pre-registered clients.
- **Tools.** The set is deliberately small:

  | Tool | Does | Human confirmation |
  | --- | --- | --- |
  | `verify_branch(repo, branch, local_sha)` | Starts §1 on the pushed head. If the remote head isn't `local_sha`, it refuses with "push first". | no (no agent, no tokens; rate-limited) |
  | `dispatch(repo, brief, ticket?, from_branch?)` | Starts a workspace lane from `main`, or from a pushed branch, on its **own** branch `refs/heads/rangelight/<lane-id>/<ticket or slug>` (inside the ref rules, ADR 0011). It never pushes to the developer's branch. | **yes** |
  | `message_lane(lane, text)` | Sends a follow-up to a lane the member started, reopening its workspace if it was closed. | **yes** |
  | `list_lanes(scope: mine \| team)` | Status of each lane, with branch, head SHA and receipt result. | no |
  | `get_lane(lane)` | Status, receipt summary, diff stats, PR link and head SHA, so the developer can `git pull`. | no |

  No MCP tool can approve, merge, stop another member's lane, or read secrets.
- **Human-action rule (threat-model §7 #22):**
  - `dispatch` and `message_lane` start or resume an agent. So they ask the member through **MCP elicitation**, which shows the repo, ticket, brief and token cap, and the member answers it, not the local agent.
  - If the client doesn't support elicitation, the lane is created as **Waiting for you** and the tool returns the Workbench link, where the member presses **Start**.
- **Limits:**
  - per-member rate limits on `dispatch` and `message_lane`
  - every lane started over MCP is tagged `source: mcp` in the event log, the Workbench and the receipt
- **What crosses from the laptop:**
  - only the brief the local agent writes, which the member sees in the confirmation, plus the repo, branch and SHA
  - never the local conversation or uncommitted files: the work must be pushed first
  - the MCP token never reaches a pod; lanes use the normal credential path (ADR 0004)

## Consequences

- **Adoption without migration:** a team can start by making `Rangelight receipt` a required check on PRs from any tool, then move work into Rangelight lanes when they want the cloud.
- **Ownership:**
  - Plan 3 owns §1's check, the external receipt and fork refusal.
  - Plan 4 owns the Check Run, the PR trigger and the MCP server.
  - Plan 5 owns the *external* lanes and **Verify a branch** in the Workbench.
- **New threat-model items:** §7 #26 and #27.
- **New residual risk:** a client that auto-accepts elicitations (§8).
- **Plan 4's first task must confirm Dex's support for the MCP authorization flow** (metadata discovery, PKCE, Client ID Metadata Documents). If Dex can't do it, the fallback is pre-registered public clients with PKCE for the documented clients.

## Revisit if

- Teams ask to verify fork PRs. Then run them in a separate, lower-trust pool with no cache writes.
- Teams want Rangelight to merge external branches. Then route them through the same merge path as lanes.
