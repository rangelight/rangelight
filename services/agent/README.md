# agent

The lane agent: one process per lane pod. It clones the repo from the git mirror, starts the
profile's dev command once, runs the unmodified `claude` binary on the ticket brief, and pushes
checkpoints and the work branch back to the mirror (ADR 0020, 0045, 0046).

| Folder | What it holds |
|---|---|
| `src/runtime/` | the `AgentRuntime` seam, its contract suite (`contract.ts`), `fake` and `cli`; the lane driver |
| `src/workspace/` | the clone (token through a git credential helper, never argv), the dev command |
| `src/checkpoint/` | checkpoint and work-branch refs, the checkpoint push |
| `src/brief/` | the ticket file and the brief |
| `src/terminal/` | the lane terminal: the PTY bridged to core NATS `rl.<org>.pty.<session>.in` and `.out` (ADR 0040 §1), signed in as the lane with its projected token (`nats_token_file`, ADR 0045 amendment), which may use only those two subjects |
| `src/subjects.ts` | every NATS subject the agent uses, checked against `api/README.md` |
| `src/events/` | the event writer: types from `shared/ts/gen/` (`scripts/check.sh gen`), validated against `api/jsonschema/agent-event.v1alpha1.schema.json`; `Clock`, `IdGen` |
| `src/config.ts` | the only environment reader: `configs/agent.default.toml`, `/etc/rangelight/agent.toml`, `RANGELIGHT_AGENT_*` |

**`cli` runtime.** Claude Code runs in a PTY (util-linux `script`) in the worktree. Its hooks
(`UserPromptSubmit`, `PostToolUse`, `PostCompact`, `Stop`) post to an HTTP endpoint on
127.0.0.1, at a random path, that the agent serves. Messages are typed at the idle prompt.
Resume is `claude --resume <session id>`, or a new session with the digest when there is nothing
to resume. `CLAUDE_CONFIG_DIR` is the lane's mounted Claude home: the agent never reads it.
The lane terminal is Claude Code's own TUI: every slash command and key works as in a local
terminal, and a turn the member types is a turn like any other. Without a login the lane asks the
member to sign in (`ask`) and runs `claude` with no prompt there; the member types `/login`, and
once `claude auth status` passes the session takes its place in the same terminal. There is no
API-key path. The PTY's output goes only to the lane terminal,
which is live and never stored or logged, so a `/login` code is never kept (ADR 0046 §3 step 4).
Git gets the mirror token only for the mirror's URL.

**Output.** stdout carries only events, one JSON object per line; dev command output and errors
go to stderr.

**Tests.** `scripts/test/services.sh run pnpm --filter agent test` (vitest against a real
nats-server; needs git, curl and util-linux `script`), or the whole TypeScript gate with
`scripts/check.sh ts`. Every runtime passes the same contract suite; `cli` is tested against
`src/runtime/testdata/fake-claude.sh`, which speaks the same flags and hooks.

**Image.** `build/package/docker/agent/Dockerfile`.
