# Rangelight

**Every ticket gets its own lane: an isolated, Kubernetes-native environment with an AI coding agent
inside it, running on your clusters, under your guardrails.**

Label a Linear ticket. Rangelight gives it a locked-down namespace, starts Claude Code in a fresh
worktree of your repo, streams every step to a live Workbench, and ends with a pull request linked
back on the ticket.

Apache-2.0. Self-hosted. Nothing leaves your cluster except the calls you allow.

> **Status: pre-alpha.** We're building v0.1 in the open. The [roadmap](#roadmap) marks what works
> today and what's coming.

## Why

AI made writing code fast. Everything around the code stayed slow:

- **Engineers** run agents in ad hoc harnesses on laptops, with real credentials and no guardrails,
  and they still fight over one shared staging cluster.
- **Platform teams** can't hand out environments as fast as agents consume them, and they have no
  paved road to offer.
- **Security** sees ungoverned agents holding real credentials, with no audit trail.
- **Product and leadership** can't see which ticket is where, or what it costs.

Rangelight is the paved road: a platform your platform team runs, so every agent works in its own
environment, inside limits Security approved, and every step leaves evidence.

## What you get

- **One lane per ticket.** Its own namespace with Pod Security `restricted`, default-deny network
  policy and its own worktree. Lanes can't see each other.
- **Linear in, pull request out.** The `rangelight` label starts a lane. The work lands as a branch
  and a PR, linked on the ticket.
- **A live Workbench.** Every lane side by side: status, the agent's feed, a terminal into the lane,
  and the PR.
- **Your Claude, your way.** Sign in with your own Claude plan through Claude Code's own `/login`
  in the lane terminal, or use a team API key. Rangelight never stores Claude credentials.
- **No credentials in agent pods.** Git access goes through a mirror that lets a lane push only its
  own refs.
- **Self-contained.** One Helm chart brings everything it needs, including Postgres and NATS. Every
  piece can be swapped for one your org already runs.
- **Evidence by design.** An append-only, hash-chained event log for every lane.

## How it works

```text
 Linear ticket ──label──▶ api (Rust) ──NATS──▶ runner (Go) ──▶ lane namespace
        ▲                  │ Postgres                          ├─ agent (TS): Claude Code in a worktree
        │                  ▼                                   └─ default-deny network policy
        └──── PR link ◀── Workbench (TS) ◀── live events          │
                                                                  ▼
                                               git mirror: the lane pushes only its own refs ──▶ GitHub PR
```

- **`api`** is the control plane. It turns tickets into sessions and keeps state, events and outbox
  in one Postgres transaction. It has no Kubernetes client.
- **`runner`** is the only component that talks to Kubernetes. It turns sessions into lanes and
  reports what happens.
- **`agent`** runs the unmodified Claude Code binary in the lane and streams typed events.
- **`git-mirror`** is stock git behind hooks. It serves each lane its own repository and rejects
  refs that aren't the lane's.

The design decisions behind each piece are recorded as ADRs in
[`docs/architecture/adr/`](docs/architecture/adr/).

## Roadmap

| Release | What it adds | State |
|---|---|---|
| **v0.1** | Linear intake, lanes, the agent with Claude sign-in, the git mirror, the Workbench, GitHub sign-in, the Helm chart | building now |
| **v0.2** | A copy of prod per lane: the services around your change, deployed through Argo CD | planned |
| **v0.3** | A signed receipt on every PR: what the agent claimed, and what Rangelight observed | planned |
| **v0.4** | Quotas, priority and preemption, warm pools, Valkey, OpenBao | planned |

## Try it

v0.1 isn't installable yet. Today you can check whether a cluster is ready for Rangelight:

```sh
RANGELIGHT_KUBE_CONTEXT=<your-context> scripts/cluster.sh preflight
```

The preflight tests behaviour, not product names. It checks that egress network policy is really
enforced, that Pod Security rejects privileged pods, that OIDC discovery is reachable and that
volumes bind.

## Develop

```sh
scripts/check.sh all        # the whole gate; CI runs exactly these parts
scripts/check.sh <part>     # size rules secrets scrub spec yaml rust go ts deploy hooks
scripts/check.sh fmt        # format in place
```

One Rust workspace, one Go module and one pnpm workspace live at the root. Every deployable is in
`services/<name>/`, every library in `shared/<lang>/<name>/`, and every cross-language contract in
`api/`. The engineering standard is
[ADR 0049](docs/architecture/adr/0049-engineering-standard.md).

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). Contributions are under the DCO; there's no CLA.

## License

[Apache-2.0](LICENSE), forever: this code is never relicensed
([ADR 0052](docs/architecture/adr/0052-open-core-boundary.md)).
