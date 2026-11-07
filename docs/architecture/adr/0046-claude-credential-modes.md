# 0046. How a lane pays for Claude: team key, plan API credits, or the member's own plan sign-in

- Status: accepted. The plan sign-in mode is **gated** on Anthropic's written confirmation (see "Open").
- Date: 2026-11-06
- Deciders: the owner
- Amends: ADR 0004 (its 2026-10-31 amendment on personal seats), ADR 0020 (the worker runtime), ADR 0034 (which traffic passes the gateway)
- Sources (checked 2026-11-03): code.claude.com/docs/en/legal-and-compliance; support.claude.com articles on using the Agent SDK with a Claude plan and on monthly API credits for Max and Team plans

## Context

Anthropic's terms, as of 2026-11-03:
- **Allowed:** an end user signing in to the unmodified Claude Code binary with their own subscription, including where a platform hosts Claude Code.
- **Required of a hosting platform:** don't remove, disable or restrict Claude Code's built-in sign-in methods.
- **Forbidden:** a "Sign in with Claude" button in our app; collecting, storing or intermediating Claude.ai credentials or session tokens; routing requests through plan credentials on behalf of users; reselling or intermediating usage.
- Developers using the Agent SDK should use API-key authentication. Since 2026-10-07, Max, Ultra and Team plans include monthly API credits, used through a key from the plan's linked Console org.

The live POC ran unmodified Claude Code on the member's plan, signed in through Claude Code's own `/login`.

## Decision

### 1. Three modes

| Mode | Who pays | Runtime | Model traffic | Caps and cost line |
|---|---|---|---|---|
| **Team API key** (default) | the org | `sdk` | through the gateway, key injected by reference | Rangelight's hard caps; dollar cost line |
| **Plan API credits** | the member's plan credits, via a key from their linked Console org | `sdk` | through the gateway, the member's key injected by reference | same as the team key |
| **Plan sign-in** (gated) | the member's own plan limits | `cli` | **direct to Anthropic**, not through the gateway | Anthropic's plan limits; the lane shows "plan usage" |

- **Keys** (both key modes) are org- or member-managed API keys: allowed by the terms, stored in OpenBao (K8s Secret references until then, ADR 0004 amendment), injected only by the gateway. They never enter a pod.
- **The dispatcher always uses the team key.** It's team infrastructure, not one person's ordinary use.
- **A plan is only used for the member's own lanes**: lanes the member started (label, Start, or MCP `dispatch` they confirmed). It's never pooled across a team, and the dispatcher can't start a lane on someone's plan.
- **The hosted tier stays bring-your-own-key or bring-your-own-plan.** Rangelight never resells inference (ADR 0004 amendment).

### 2. Two agent runtimes behind one seam

`AgentRuntime` (ADR 0049 §2.2) has two implementations:

| | `sdk` | `cli` |
|---|---|---|
| What runs | the Agent SDK driving Claude Code | the unmodified `claude` binary in the lane's PTY |
| Messages (ADR 0045) | typed streaming input at turn boundaries | typed messages rendered as a prompt at the next idle prompt, detected by Claude Code's hook events |
| Events | SDK message stream | Claude Code hooks (`PostToolUse`, `Stop`, …) writing to `rld` |
| Resume | SDK resume by session ID | `claude --resume <id>` |
| Used by | both key modes, the dispatcher | plan sign-in |

Both write the same events to `rld` and produce the same receipt. The receipt records `runtime` and `auth` (`team-key`, `member-key`, `claude-plan`), the member for the plan modes, and never a token.

### 3. Plan sign-in, end to end

1. The member chooses "Your Claude plan" in their own settings (never an admin choice made for them).
2. Their lane's pod starts with egress to the gateway, mirror and NATS, **plus Anthropic's API and sign-in hosts** for this lane only. Hostname-based egress needs an FQDN-aware policy (Cilium `CiliumNetworkPolicy` `toFQDNs`); plain NetworkPolicy can't express it. **Plan sign-in is therefore available only on clusters with Cilium**, and the preflight reports it.
3. The lane goes `waiting_for_you`: "sign in to Claude". The member opens the lane's terminal and runs Claude Code's own `/login`. Claude's page opens in their browser. **Rangelight shows no sign-in button of its own.**
4. While the login runs, **terminal recording is paused** and the PTY stream isn't written to the event log, so the pasted code never lands in our storage. The code is useless outside the pod (PKCE).
5. Claude Code stores its credential in its own store inside the pod. **Rangelight never reads, copies or moves it.** It is destroyed with the pod.
6. When the plan's limit is hit, the lane goes `waiting_for_you` with two choices: wait, or switch this lane to the team key (which restarts the runtime as `sdk` with the same session digest).

**Signing in once per member** (a per-member Claude home volume that only that member's lanes mount, so new lanes start signed in) is **off** until Anthropic confirms it, because the credential file would sit on our disk. Until then, every new pod means a new sign-in, and that friction is why "plan API credits" is the recommended mode for plan holders.

### 4. What this changes in ADR 0004

"No raw credential ever enters an agent pod" stays the rule for every credential **Rangelight** holds. Plan sign-in is a narrow, explicit exception: the credential is the member's own, created by Claude Code's own flow inside the pod, scoped to that member's lane, never seen by Rangelight, and destroyed with the pod.

## Open

- **Anthropic's written confirmation** that (a) plan sign-in with Claude Code hosted in a lane is ordinary use, and (b) the per-member home volume is acceptable. The email is drafted (`docs/drafts/2026-11-03-anthropic-plan-signin.md`) and not sent. Until (a) is confirmed, plan sign-in ships disabled by default and labelled "pending confirmation". If the answer is no, the mode is removed and plan holders use plan API credits.

## Consequences

- **Plan 1** builds the `AgentRuntime` seam with `sdk` only.
- **Plan 5** builds the member's "How I pay for Claude" setting (team key, my plan's API credits, my plan sign-in) and storing a member key as a Secret reference.
- **Plan 6** injects member keys in the gateway and enforces caps per credential.
- **Plan 5** also builds `cli`, the sign-in flow in the lane terminal, the paused recording and the limit-hit choice.
- Threat model §7 #29 covers plan sign-in.
- The Workbench mock-up's "Claude access" view (`g c`) is the reference UI.

## Revisit if

- Anthropic changes its terms. Re-check the sources above at each release.
- Another agent vendor allows hosted plan sign-in. Then its `cli`-style runtime follows the same rules.

## Amendment (2026-11-06)

- **Scratch terminals use the `cli` runtime in every mode** (ADR 0055). In the key modes, its traffic goes through the gateway with the key injected there.

## Amendment (2026-11-07, owner decision: plan sign-in first, with a persistent Claude home)

- **Plan sign-in comes first, in Plan 1, for the owner's own lanes.** No API key is used in Plan 1. The owner's reason: lanes should run on the owner's subscription, and this is the lowest common ground for now.
- **The per-member Claude home is on for the owner, on the dev cluster,** although §3 kept it off until Anthropic confirms it. **Accepted risk (owner):** the Claude Code credential sits on the cluster's disk, and Anthropic hasn't confirmed this in writing. The drafted email (RAN-13) matters more now; until an answer arrives, the home stays owner-only, and no other member gets one.
- **How:** an owner-applied StorageClass `rangelight-claude-home` (local-path, `reclaimPolicy: Retain`, path `claude-home/<member>`) maps every lane's claim with label `rangelight.dev/member=<member>` to one host folder, mounted as `CLAUDE_CONFIG_DIR`. The member runs Claude Code's own `/login` once, in the first lane. Rangelight still never reads, copies or moves the credential.
- **The builder never reaches it** (the `rangelight-agent-guard` policy, RAN-12): the builder can't create claims of that class; in a namespace labelled `rangelight.dev/claude-home=true` it can only port-forward (no exec, attach, pods or claims); it can't relabel or delete such a namespace. Only the runner creates those claims, and the runner moves to the owner-applied namespace `rangelight-runner`. Every claim of the class must carry a valid member label.
- **Runtimes:** Plan 1 builds the `cli` runtime (the unmodified `claude` binary in the lane's PTY, events from Claude Code's hooks). `sdk` moves to the key modes (Plans 5 and 6). The gateway's proxy and meter are still built in Plan 1 and tested against a mock upstream, but not used live.
- **Egress:** lanes reach Anthropic's hosts through a Cilium `toFQDNs` policy (§3 step 2); the host list comes from Claude Code's documented network requirements at implementation time.
- **Reverse if:** Anthropic says the hosted or persistent home isn't allowed. Then the volume is removed and every pod signs in again, or the owner uses plan API credits.
