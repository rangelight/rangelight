# 0020. Agent runtime: one TS runner per session; lanes are short-lived SDK subagents

- Status: accepted
- Date: 2026-10-30
- Deciders: the owner

## Decision

- **One TypeScript runner process per session** wraps the Claude Agent SDK and hosts the coordinator.
- **The coordinator is a dispatcher, never a worker:**
  - It doesn't read files, logs or diffs itself.
  - Each turn it is rebuilt from a compact session-state digest plus the relevant recent events.
- **Lanes are SDK subagents:**
  - spawned per task (per repo or per role: QA, Security, …)
  - each with its own context window
  - with scoped tools and permissions
  - returning a **size-capped, typed result**, not prose
  - ended when their task completes
- **Big artifacts go by reference:** logs, traces, diffs and peer transcripts enter context as IDs plus a summary, with a retrieval tool for slices.
- **Cross-repo work:** a lane that needs another repo's toolchain executes in that repo's container in the same pod (ADR 0013).
- **Separate processes** only where a lane needs a different permission boundary.
- **Compaction is a fallback.** The PreCompact hook persists the summary as an event, so the receipt records that compaction happened.

## Consequences

- One prompt-cache prefix per session (system prompt, team skills, locked contract).
- Context size per agent per turn is emitted as a metric, with a budget.

## Revisit if

- SDK subagent isolation, compaction or permission scoping proves insufficient. Re-verify the current SDK behaviour before implementing.

## Amendment (external review, 2026-10-31)

- **Use the SDK's own session model** (resume, plus auto-compaction with the PreCompact hook) instead of rebuilding the coordinator from scratch every turn.
- **The session-state digest is *injected* as context** on resume and after compaction.
- Re-verify the SDK's behaviour before implementing.
- **The adapter for other agents is not a public contract** until a second agent exists (ADR 0025 amendment).

## Amendment (2026-11-02: lanes are workspaces)

- **Lanes are workspace pods, coordinated by the team dispatcher** (ADR 0044, items 6 and 11). This supersedes in-pod lanes as SDK subagents.
- **Each workspace runs one SDK session** with native resume and auto-compaction, the digest injected on resume, and **one optional reviewer subagent**.
- **The dispatcher** is a long-lived SDK session per Linear team. It never touches code, its only tools are Rangelight API calls, and it reads state from Postgres every turn.
- Cross-workspace coordination goes through the dispatcher and the `api`, never pod to pod.
- The POC checks (Plan 1) still verify SDK subagent scoping, hooks and resume, because the reviewer subagent and the dispatcher depend on them.

## Amendment (2026-11-06)

- **Two runtimes behind `AgentRuntime`** (ADR 0046): `sdk` (key modes, the dispatcher) and `cli` (unmodified Claude Code in the PTY, for plan sign-in). **Messages reach a lane at turn boundaries** from a Postgres mailbox, delivered over NATS and acked after injection (ADR 0045 §2).
