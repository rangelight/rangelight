# 0008. Capacity is first-class: admission, quotas, priority, preemption by hibernation, approved bursts

- Status: accepted. The engine choice is pending verification (Kueue's fit for long-lived interactive sessions).
- Date: 2026-10-29
- Deciders: the owner

## Context

The owner's buy-ASAP wedge has three parts:
- dynamic per-ticket environments with the agent inside the same namespace
- an amazing Workbench
- quotas at org, team and engineer level, with high-priority tickets and bugs jumping the queue

On-prem clusters have fixed capacity. Once every ticket gets an environment, environments become the scarce resource.

## Decision

- **Topology confirmed.** One namespace per session holds the agent pod, the previewed services and the data stores. Traffic stays inside the namespace. Egress is default-deny except to Rangelight's gateways.
- **Admission.** A session starts immediately or waits in a queue. The Workbench shows its queue position and an ETA.
- **Quotas in two currencies, with org → team → engineer limits:**
  - compute: concurrent hot sessions, CPU, RAM and GPU
  - money: LLM spend and estimated compute, enforced through the usage ledger
- **Priority** is derived from the ticket (P0, prod bug, …) plus team policy. A high-priority session goes to the front of the queue.
- **Preemption by hibernation.** If there is still no room, the victims move from hot to warm (ADR 0007): pods are removed, and the session stays rebuildable (ADR 0010).
  - Victims are chosen in policy order: idle first, then lowest priority, then youngest.
  - No work is lost.
  - The owner gets a Decision card, and the session resumes automatically when capacity frees up.
  - Both sessions' receipts record the preemption.
- **Burst.** Admins (or whoever the policy delegates to) can approve a time-boxed burst over quota through a Decision card. The burst can come from:
  - borrowing idle capacity from other teams (reclaimed on demand)
  - triggering node autoscaling
  - accepting overcommit

  Every burst has an expiry, an owner and a cost line in the ledger.
- **The engine is Kubernetes-native**, with Kueue as the leading candidate (ClusterQueue/LocalQueue, cohorts, borrowing, WorkloadPriorityClass, preemption). Rangelight maps preemption to *hibernate* instead of *evict*.
  - To verify: Kueue's support for long-lived, plain-Pod or custom-CRD workloads, and per-user quotas, which may need a LocalQueue per user or a Rangelight admission layer.

## Consequences

- Scheduling and fairness rules become part of the per-team config (ADR 0005), with org floors.
- "Queued" is a real session state in the UI and the event log.
- Hibernate and wake must be fast and reliable, because they are now in the preemption path.

## Revisit if

- Kueue can't model interactive sessions cleanly. In that case, build a Rangelight admission controller on the same concepts.

## Amendment (2026-10-29): what is quota'd, and lessons from Coder

- **LLM usage is not part of a Rangelight per-engineer quota.**
  - LLM capacity comes from the user's own subscription seat, where the vendor enforces the limits (ADR 0004 personal-seat mode).
  - It also comes from org API keys or cloud credentials, with pre- or post-approved budgets, metered in the ledger.
  - **Per-engineer quota applies to environments**, because they are the bottleneck and get expensive fast.
- **Prior art: Coder** (`coder/coder`, AGPL-3.0, docs read 2026-10-29):
  - **Quotas:** credits per day are attached to template resources (`daily_cost`), and running and stopped resources cost different amounts. A user's budget is the sum of their groups' allowances, computed per org. When the budget is exceeded, new workspaces are blocked. *(This is Premium.)*
  - **Lifecycle:** autostop on inactivity, an "activity bump", dormancy followed by auto-delete, failure cleanup, and a forced restart to apply template updates. *(Dormancy and failure cleanup are Premium.)*
  - **Prebuilt workspaces:** a pool per parameter preset, reconciled continuously, with TTLs and time-of-day schedules. *(This is Premium.)*
  - **AI:**
    - The agent loop runs in the control plane, and workspaces are network-isolated.
    - The AI Gateway handles prompt and token audit and MCP allowlists.
    - The Agent Firewall controls egress at the process level.
    - *(All three are Premium. The Community edition caps concurrently active agents at 5.)*
- **Rangelight takes:**
  - credit-style costs per *environment state*, with hot billed higher than warm
  - group-derived allowances
  - inactivity autostop (hot → warm) and dormancy (warm → cold, with the receipt kept for replay)
  - a scheduled warm pool

  **All of it is free** (ADR 0003's OSS line).
- **Rangelight differs:**
  - Environments are per *ticket* and prod-shaped (Argo previews), not per developer and long-lived.
  - Every action lands in a receipt.
- **The quota unit is hot slots plus env-hours** (decided).
  - An engineer sees concurrent hot slots (for example 3) and a weekly env-hour allowance.
  - Usage is weighted by profile size (a large profile burns 2×). Warm hours cost a fraction.
  - Dollars appear only in the team, manager and FinOps views, through the operator-set $/vCPU-hour.
  - Allowances come from groups (teams), with org floors and caps in config (ADR 0005).
