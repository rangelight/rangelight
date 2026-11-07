# 0057. Every seat: role homes, free viewers, and a view for each job around a change

- Status: accepted
- Date: 2026-11-06
- Deciders: the owner, on the CPO and UX review
- Amends: the roadmap (Plans 5, 7, 9, 11), ARCHITECTURE.md §9 and §10, ADR 0028 (Cedar roles), ADR 0052 §4 (free-forever list)

## Context

- The plan serves one seat well: the engineer running lanes.
- Everyone else around a change has to read a diff to learn anything, or isn't served at all:
  - the reviewer
  - platform and DevOps
  - security
  - the product manager
  - budget and FinOps
  - customer success
  - engineering leadership
- In most orgs, those seats decide whether a tool is adopted.

## Decision

### 1. A home for each seat, and free viewers (mockup now; product in Plan 5)

- **What it is:** the Workbench opens on **Home** (`g h`), the screen for the person's seat. Each home answers one question; everything else is a click away.

  | Seat | The question | Writes? |
  |---|---|---|
  | Engineer | What needs me, and what is running? (the Architect stream) | yes |
  | Reviewer | What can I approve with confidence? | yes |
  | Platform | Is the factory healthy, and does it have room? | yes |
  | Security | What did agents try that policy stopped? | yes |
  | Product | Is my ticket done, and does it do what I asked? | viewer, plus preview sign-off |
  | Budget | Are we on budget, and what does a verified merge cost? | viewer |
  | Customer success | Is the customer's fix verified, and is it live? | viewer |

- **Roles:** seats map onto Cedar roles (ADR 0028). The new role is **`viewer`**: read-only, with no lanes and no scratch terminals.
  - A PM's preview sign-off is its own narrow permission (§2).
- **Viewers are free and unlimited.** No per-seat pricing for viewers, in the open-source product or the hosted tier.
- **Mockup:** `src/views/RoleHome.tsx` and `src/seat/seat.ts`. Viewer seats hide write actions.

### 2. Plain-language summary and preview sign-off (v0.2, Plan 9)

- **The summary:** every receipt gets a plain-language summary: what changed, which criteria are proven, the preview link and screenshots.
  - It's built **from the receipt's fields**, never from the agent's own claims.
- **Sign-off:** a PM's **Accept** on a preview is recorded in the receipt as human evidence with provenance **`attested`**, never `observed`. It names the person and the preview commit.
- **Release notes** are generated from the merged receipts in a release.

### 3. Insights and Budgets (v0.2, Plan 7, with the views in Plan 5)

- **Insights:**
  - verified merges per week (the headline metric, ADR 0056 §6)
  - claim-gap rate
  - cycle time from ticket to verified merge
  - cost per verified merge
  - gate p90
- **Budgets:** per team and per project, over the existing hard caps.
  - a simple forecast (run rate to period end)
  - alerts at 50, 80 and 100%
  - CSV export
- The data is already in Postgres (the cost line, ADR 0034), so this is views and one alert job.

### 4. The Security view (v0.2, Plan 7; the view in Plan 5)

- **What it shows:**
  - a live feed of **blocked actions**: denied egress, refused pushes, credential attempts, tools not granted
  - audit search
- **Audit export** to the org's SIEM, as OpenTelemetry logs or syslog.
- **Policy dry-run:** "would this lane be allowed to X?" against the effective Cedar policy. This was "target" in ARCHITECTURE.md §9; it moves into v0.2.
- **All of this is free forever** (ADR 0052 §4): security basics are never gated.

### 5. Slack, then Teams (v0.2, Plan 5)

- **In Slack:**
  - decision cards with their option buttons
  - lane status in a channel
  - a weekly digest per seat
- **Starting a lane from Slack** still needs a member's explicit action, and the member's identity is linked through OIDC. A Slack message from someone else never starts one.
- **The same identity rule applies to** the web push of ADR 0056 §4: every answer is an authenticated action.

### 6. Ops pack in the chart (Plans 7 and 11)

- Grafana dashboards and Prometheus alert rules ship in the Helm chart.
- **Every alert has a runbook** in `docs/runbooks/`, linked from the alert.
- The **Platform home** shows hot slots, the queue, gate p90 and preview sync.

### 7. A status page for customer success (v0.2, Plan 9)

- **What it shows:** a shareable page per ticket, in plain words: fixed? verified? which version, deployed where?
- **Where it comes from:** the receipt and the deploy state, through the same share-link plumbing as §2.
- **Who can see it:** a link is scoped to one ticket and expires. Viewing it needs a viewer seat, or an explicit org setting for unauthenticated links.

### 8. UX quality gates (from v0.1)

- **Accessibility:** WCAG 2.2 AA, checked by axe in CI on every view, plus keyboard-only paths for every action. A keymap already exists.
- **First run:** a guided start with the demo org (the existing story mode), and real empty states for every view.
- **Copy:** every user-facing string is externalised, ready for translation.
- **A performance budget** for the Workbench's first load, checked in CI.

### 9. Not added, on purpose

- dashboards with no single question
- per-seat pricing for viewers
- paywalled security basics
- a native mobile app (web push covers the phone)

## Consequences

- More people can use Rangelight without touching a cluster, which is the adoption lever for the seats that decide.
- The receipt gains a fourth provenance class, `attested`, alongside `observed`, `claimed` and `reported`. The verifier and the summary must keep it separate.
- The Workbench gains seven home screens. The rule "one question per home" is what keeps them small.

## Revisit if

- Design partners don't use a home. Then remove it.
- A seat asks for writes it doesn't have. Then add narrow permissions, never a wider role.

## Amendment (2026-11-07, seat order)

- **Owner's decision:** Engineers, Platform/DevOps, Security and Tech Leads get value end to end first. The Product, Budget and Customer-success seats come **after v0.2.0**, in a new Plan 12.
- **Moves to Plan 12:**
  - the Product, Budget and Customer-success homes and the free `viewer` role (§1)
  - the plain-language summary, PM preview sign-off (`attested`) and release notes (§2)
  - Budgets: forecast, 50/80/100% alerts, CSV (§3); the hard token caps stay in Plan 6
  - the customer-success status page (§7)
- **Stays in v0.1/v0.2:** the Engineer, Reviewer, Platform and Security homes; Insights for Tech Leads (§3); the Security view, SIEM export and policy dry-run (§4); Slack decision cards and lane status (§5); the ops pack (§6); the UX gates (§8).
- **Why:** the champion and daily users decide adoption. Business views are only as good as the receipts and cost data under them, which are real only after v0.2.0.
- **ARCHITECTURE.md §10:** the v0.2.0 "Every seat" bullet now covers only what stays above.
- **Revisit if** a design partner's buyer is a PM or FinOps lead and asks for one of these views before v0.2.0.
