# 0045. Workspace lifecycle and the lane contract: messages, Rangelight's own checks, close and reopen

- Status: accepted
- Date: 2026-11-06
- Deciders: the owner
- Amends: ADR 0010 (hibernate becomes "close"), ADR 0020 (2026-11-02 amendment: how the dispatcher and lanes exchange messages), ADR 0021 (who holds a lane's NATS credentials), ADR 0041 (what the observed gate checks)
- Evidence: the live POC on `poc/live-lanes` (commits `d2f14cc`, `cf3b7c1`)

## Context

ADR 0044 made each ticket a workspace lane, coordinated by the team dispatcher. It didn't say how messages reach a running lane, what "done" means, or what happens to a lane's pod and evidence once work stops. The POC answered all three in practice:
- Lanes take follow-up messages and resume the same agent session.
- "Done" is decided by Rangelight's own checks, not the agent's summary.
- Closing the pod must never lose work, and the diff, receipt and transcript must stay viewable after the pod is gone.

## Decision

### 1. Lane states

```
queued → starting → running ⇄ waiting_for_you
                       ↓
                   verifying → done (ready for review) → merged
                       ↓            ↓
                    failed ←────────┘ (re-check against a new main fails)
any state → stopped (by a member)
```

- **The state lives in Postgres** (ADR 0031). The `Session` CR mirrors it (ADR 0012).
- **`waiting_for_you`** means the lane needs a member: an `ask`, a sign-in (ADR 0046), a Decision card, or a `dispatch` awaiting confirmation (ADR 0048).
- **The pod is a separate axis:** `live`, `closed` or `reopening`. A lane in any state except `starting` and `running` may have a closed pod.

### 2. Messages to and from a lane

- **Postgres is the mailbox of record; NATS only delivers.** A message exists once its `lane_message` row is committed. The outbox publishes it to `inbox.session.<id>` (a durable JetStream consumer per session; permissions in threat-model §7 #10).
- **Senders:** a member (Workbench composer, Linear comment that mentions the lane), the dispatcher, or MCP `message_lane` (ADR 0048). All go through the `api` with Cedar, rate limits and the audit log. **Nothing publishes to a lane's inbox directly.**
- **Injection at a turn boundary.** The lane injects a message only between turns, never mid-tool-call. It acks only after the message is in the agent session. A crash means redelivery; the lane dedupes on `message_id`. Delivery is at-least-once, and no message is lost.
- **From the lane, only typed messages:**

  | Message | Fields (size-capped) | Effect |
  |---|---|---|
  | `ask` | `question`, `options[]?` | lane → `waiting_for_you`; the dispatcher and the lane's owner see it |
  | `worker_done` | `summary` (≤ 600 chars), `head_sha` | starts Rangelight's checks (§3) |
  | `progress` | `stage`, `note` (≤ 200 chars) | feed only |

  Free-text channels from a lane to other lanes don't exist. Lanes never talk to each other; coordination goes through the dispatcher and the `api` (ADR 0020 amendment).
- **Waking the dispatcher.** Deterministic `api` rules publish a batched wake to the dispatcher's inbox on: `ask`, `worker_done`, a check result, a failed lane, a ticket change, and a lane idle past its threshold. The dispatcher doesn't poll.

### 3. "Done" means Rangelight checked it

On `worker_done`, the runner starts the **observed gate Job** (ADR 0041) on the pushed head. It runs in its own pod with a read-only checkout and no credentials. The agent can't write to it. The checks, from the POC:

| Check | Passes when |
|---|---|
| Tests pass in Rangelight's own run | the repo's test command runs on the pushed head and every test passes |
| Existing tests on `main` still pass | every test that passes on `main` also passes on `main` merged with the branch (not on the branch alone, which may be behind) |
| Committed and pushed | no uncommitted changes, at least one commit, and the verified SHA equals the remote head |
| Merges cleanly into `main` | `git merge-tree` reports no conflicts |

**Criteria are proven only by tests the branch added.** Each acceptance criterion `n` is proven by a test whose name contains `AC<n>` that **doesn't exist on `main`** and passes in Rangelight's run. Otherwise it's `failed` (the test exists and fails) or `claimed` (no such test). The agent's brief tells it this naming rule.

- The test command and its output format (TAP or JUnit XML) come from the `rangelight.yaml` `tests` section, read from the base branch (ADR 0041).
- The result is the receipt's `checks[]` and `criteria[]` (receipt predicate v1alpha1). The lane becomes `done` only if every check passes and no criterion is `failed`.
- **No approval while a criterion is only `claimed`.** The Workbench shows the gap and the reviewer can send the lane back with one message (ADR 0048 applies the same rule to external branches).
- **When `main` moves**, the `api` re-runs the checks for every `done` lane against the new `main`. A lane whose checks now fail goes back to `failed`, with the reason.

### 4. Close, snapshot, reopen

- **Closing removes the pod; it never removes work.** Before the pod goes, the lane:
  1. commits any uncommitted changes as a `wip:` commit and pushes it to the lane's branch on the mirror
  2. saves the diff against `main` and the session transcript to object storage (RustFS, S3 API), referenced by digest from a `lane_snapshot` row
  3. emits `workspace_closed` with the reason
- **When the pod closes:**
  - `merged` or `stopped`: immediately
  - `done` or `failed` or `waiting_for_you`: after **30 minutes idle** (configurable per team, `lanes.idle_close_minutes`)
  - any lane when a hot-slot is needed for a P0 start (ADR 0042), oldest idle first
- **After it closes,** the Workbench serves the diff, receipt, feed and transcript from the snapshot and Postgres. The lane shows "workspace closed HH:MM".
- **Reopening:** a message, the Terminal, or a re-check on a closed lane rebuilds the pod from the lane's branch (prebuild + worktree, ADR 0024). The agent **resumes the same session** (SDK resume, ADR 0020) and receives the digest. If resume isn't possible, it starts a new session with the digest and the last summary, and the feed says so.
- **Teardown** (namespace, refs, snapshot retention) follows the retention policy; the receipt and events bundle outlive all of it (ADR 0042).

### 5. The `rld` sidecar holds the lane's Rangelight credentials

The model's Bash runs as the same user, in the same container, as the agent runner. Anything the runner holds, the model can read. So the lane's NATS credentials **don't** live in the agent container:

| Container | Holds | Runs as |
|---|---|---|
| `agent` | the agent runtime (ADR 0046), the worktree, the toolchain; the gateway's projected token | uid 1000, no NATS credentials |
| `rld` | the session's NATS user JWT (ADR 0021, §7 #10), the inbox consumer, event publishing | uid 1001; serves the agent over a Unix socket on a shared `emptyDir` |

- `rld` validates every outgoing message against its typed schema and size cap (§2). The model can call the socket, but it can only say what the schema allows, and every message is attributed.
- `rld` acks inbox messages only when the runtime confirms injection (§2), so the model can't consume its own inbox.
- `rld` is written in Rust (`control-plane/services/rld`, its own image), so the protobuf types and validation are shared.

## Consequences

- **Plan 1** checks the SDK behaviours this depends on: streaming-input injection at a turn boundary, resume in a new pod, and cancel (Task 12).
- **Plan 2** owns the lane state machine, the mailbox tables, the wake rules and the close/reopen jobs.
- **Plan 3** owns the gate Job checks and the `checks[]`/`criteria[]` receipt fields.
- **Plan 5** shows closed lanes, the claimed-criterion gap and the send-back message.
- Threat model §7 #28 covers the sidecar boundary.

## Revisit if

- A repo's tests are too slow to run twice (branch and merged). Then the baseline on `main` is cached per `main` SHA, which the POC didn't need.
- Teams want criteria proven by something other than named tests (Playwright traces, screenshots). Then extend the criteria rule; `observed` stays the only proof.

## Amendment (2026-11-06)

- **The org's own CI also gates the lane** (ADR 0054): a lane is `ready` only when Rangelight's checks and every required `externalChecks` entry succeed. A post-merge verification reuses the receipt when the merge tree equals the verified head's tree.

## Amendment (2026-11-12, RAN-43: the lane's terminal identity)

- **The agent container holds one NATS identity: its terminal's** (a projected token, audience `rangelight-nats`).
  It may only subscribe to `rl.<org>.pty.<session>.in` and publish `rl.<org>.pty.<session>.out` (ADR 0021
  amendment): the terminal the pty user already has, so the model gains nothing from reading it. §5 still holds
  for everything else: the session's broader identity (its inbox, its events) lives in `rld` when `rld` comes.
