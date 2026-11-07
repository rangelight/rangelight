# Linear: how tickets work

How the Rangelight team uses Linear. The decisions are [ADR 0050](../architecture/adr/0050-delivery-practices.md)
(§4 to §6 and its amendments); this page is the working setup.

## Team and keys

- Team key `RAN`: tickets are `RAN-<n>`, and code markers are `TODO(RAN-<n>): ...`.
- Every ticket is in a project, and every project has a target gate (an adoption gate, ADR 0044),
  not only a date.
- Weekly cycles, with a 30-minute review of the numbers from `script/metrics.sh weekly`.

## Labels

One small set. The names are flat (no label groups), so tools read `kind/feature` as written.

| Label | Use |
|---|---|
| `area/<component>` | `area/spec`, `area/control-plane`, `area/runner`, `area/agent`, `area/web`, `area/chart`, `area/git-mirror`, `area/ci`, `area/docs` |
| `kind/feature` | new behaviour |
| `kind/bug` | wrong behaviour, with a reproduction |
| `kind/spike` | time-boxed research; the output is a document |
| `kind/chore` | upkeep with no behaviour change |
| `kind/security` | closes or tests a threat-model item |
| `needs-owner` | waits on a decision only the owner can make |
| `blocked` | waits on something outside the ticket; the description says what |

No date labels and no synonym pairs. A new label is a change to this page.

## Templates

Each template carries the acceptance-criteria block. Criteria are numbered, and each one is
checkable by a test named after it (`AC1`, `AC2`, ...).

**Feature**

```
## Why
## What
## Acceptance criteria
- AC1: ...
- AC2: ...
## Out of scope
## Links (ADR, threat-model items)
```

**Bug**

```
## Reproduction (steps, expected, actual)
## Acceptance criteria
- AC1: the reproduction passes, as a regression test
## Links
```

**Spike** (time-boxed)

```
## Question
## Time box
## Output (the document this produces, and where it lives)
## Acceptance criteria
- AC1: ...
```

## Changing a ticket

- **Acceptance changes are edits to the description**, with a dated line: `Revised YYYY-MM-DD: ...`.
  Never a comment. A change after work starts moves the ticket back to Todo.
- **One ticket, one pull request**, even across components.
- A ticket created after a plan starts goes to the backlog. It joins the plan only by a decision
  recorded on the plan's ticket.

## States move by automation

| Event | State |
|---|---|
| A branch or pull request is linked | In Progress |
| The pull request is ready for review | In Review |
| The pull request is merged | Done |

Bulk state edits by hand aren't allowed. Branches are named `<type>/RAN-<n>-<word>-<word>`
(`feat`, `bug`, `hotfix` or `release`; upper-case `RAN`). Linear's "copy git branch name" gives
lower case, so build the name from the ticket ID instead. Linear still links it, because its
matching ignores case.

## WIP limits

- At most **3 tickets In Progress per actor** (the owner, or the agent identity).
- At most **5 tickets In Review** in total.
- Over a limit, nothing new starts: finish or review first.

## Priority

- **Urgent** only with a running incident or a broken `main`.
- **High** is at most 20% of open tickets. The weekly review rebalances.

## Owner decisions

- Decisions only the owner can make get the `needs-owner` label and are batched once a day in one
  view, never asked inline in a comment thread.
- A ticket waiting on the owner for more than 2 working days is raised in the next daily batch.

## Who did what

Agent comments and state changes go through Linear's app actor for the agent identity, so the
actor is in the metadata, not in the text.
