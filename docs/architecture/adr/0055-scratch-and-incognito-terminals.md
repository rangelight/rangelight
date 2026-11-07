# 0055. Scratch terminals: a throwaway pod with Claude Code, optionally incognito

- Status: accepted
- Date: 2026-11-06
- Deciders: the owner
- Amends: ADR 0046 (the `cli` runtime is also used outside plan sign-in), ARCHITECTURE.md §9

## Context

- Sometimes a member wants a terminal with Claude Code, not a lane: to try something, ask a question about the code, or get a second opinion. That means no ticket, no contract, no PR.
- Sometimes they want **fresh eyes**: a Claude that hasn't absorbed the project's instructions, memory and history. Claude's own apps call this incognito.
- Browsers reserve ⌘T for a new tab, so a web page can't use it.

## Decision

1. **A scratch terminal is a throwaway pod.**
   - **Starting one:** the button in the top bar, **⌥⌘T** (Alt+Ctrl+T elsewhere), or "New scratch terminal" in the ⌘K palette. Each press opens one more; they dock as tabs.
   - **The pod:**
     - the same hardened pod and namespace model as a lane (`rl-<session>`), from the repo's prebuild
     - the repo checked out **read-only**, with no push, no PR and no Linear sync
     - the unmodified `claude` binary in a PTY (the `cli` runtime, ADR 0046)
   - **Model traffic:**
     - In the key modes, it goes through the gateway with the key injected there. No key ever enters the pod.
     - In plan sign-in (still gated), it's the member's own plan.
   - **When it ends:** closing the tab deletes the pod. An idle timeout (default 30 minutes) deletes it too.
   - **Admission:** it takes a hot slot like any session (Plan 7).
   - Only a member's own click or keystroke starts one, never the dispatcher or an agent.
2. **Incognito** (**⇧⌥⌘T**, or Shift-click the button): the same pod, started with **no project context**:
   - The checkout leaves out the instruction files (`CLAUDE.md`, `AGENTS.md`, `.claude/`, and other agents' rule files) through sparse-checkout exclusions. The code itself is there.
   - It starts from an empty Claude Code config directory: no memory, no user or team settings, no skills, plugins, hooks or MCP servers.
   - It doesn't resume or see past sessions, and the dispatcher's notes aren't injected.
   - Its transcript and terminal output aren't stored. Rangelight keeps only the audit line: who, when, how long, and the cost.
   - **The test:** a canary sentence in `CLAUDE.md` and in a memory file, which an incognito session must not be able to quote.
3. **A normal scratch terminal** loads the project context like a lane does. Its transcript is sealed (ADR 0030) and deleted with the pod.
4. **Scope:**
   - The mockup has the button, the shortcuts, the palette entries and the dock now.
   - The real pod comes with the Workbench terminal in Plan 5, which already has the PTY over the `api` WebSocket.
   - **Promoting a scratch terminal to a lane is not planned.** Add it when someone asks.

## Consequences

- Fresh-eyes reviews are one keystroke away and can't leak the project's framing into the answer.
- Incognito has an audit line but no content. Org admins can still see usage and cost.
- The `cli` runtime now runs in the key modes too, so the gateway must accept Claude Code's own traffic, not only the SDK's.

## Revisit if

- Claude Code gains a built-in flag that skips all project and user context. Then use it instead of the empty config directory and the exclusions, and keep the canary test.
