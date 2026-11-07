# 0047. Repo onboarding: the GitHub App's selection is the source of truth; mirror fetches are control-plane jobs

- Status: accepted
- Date: 2026-11-06
- Deciders: the owner
- Amends: ADR 0011 (who fetches from upstream), ADR 0040 (GitHub App events), ADR 0024 (when the first prebuild runs)

## Context

There was no admin flow for adding a repo. ADR 0011 also contradicts itself: the mirror "holds no secret", yet it runs `git fetch` from upstream, which for a private repo needs the GitHub App installation token. The POC used one hard-coded repo.

## Decision

### 1. GitHub decides which repos are connected

- The admin selects repos on the **GitHub App's own install or configure page**. In Rangelight, Settings → Repos → **Add repo** links there. Rangelight keeps no second list to drift.
- The `installation` and `installation_repositories` webhooks (plus a reconcile poll every 10 minutes, ADR 0040) tell the `api` which repos were added or removed.
- **Which teams may use a repo** is Rangelight config (`rangelight-config`, ADR 0005): `repos.<owner/name>.teams`. A repo with no team mapping is mirrored but can't start lanes. Default for a single-team install: all repos to that team. **Until the config pipeline lands (Plan 7), Plan 4 reads the mapping from the `api`'s own config file** (`repos.default_team`), and Plan 7 moves it to `rangelight-config` unchanged.

### 2. Onboarding a repo (a durable `api` job, ADR 0040)

1. **Initial clone** into the mirror's volume (§3).
2. **Read `rangelight.yaml`** from the default branch and validate it against the schema. Missing → the repo shows "no `rangelight.yaml`" with a generated starter file to copy. Invalid → the schema errors.
3. **First prebuild** (ADR 0024) from the default branch.
4. Status `ready`. Lanes may start.

Removing a repo from the App stops new lanes immediately, lets running lanes finish, and archives the mirror after the retention period.

### 3. Fetching is a control-plane job, not the mirror's

- **The fetch job** runs in `rangelight-system` as its own short-lived Job, mounting the mirror's volume. It gets an installation token through a git credential helper that asks the `api`. The token lives only in that Job's memory for its run. **The mirror Deployment holds no upstream credential**, and the ADR 0011 contradiction is resolved.
- **Explicit refspecs only:** `+refs/heads/*:refs/heads/*` and `+refs/tags/*:refs/tags/*`, with `--prune` scoped to those. **Never `--mirror` or the default refspec**, which would delete every `refs/rangelight/*` session ref on each sync.
- **Triggers:**

  | Trigger | Action |
  |---|---|
  | `push` webhook | fetch, debounced 10 s per repo |
  | every 5 minutes | backstop fetch |
  | `main` moved | re-check `done` lanes (ADR 0045), rebuild the prebuild if its inputs changed (ADR 0024) |
  | nightly | `git maintenance run --task=gc --task=commit-graph` |

- **Pushing upstream** and opening the PR stay a separate `api` job (ADR 0011).

### 4. What admins see (Settings → Repos)

For each repo: mirror status (`cloning`, `synced <time>`, `error`), size, default branch and its SHA, `rangelight.yaml` found / missing / invalid, last prebuild and its digest, last fetch error, and the teams that may use it. The mock-up's Repositories view (`efa2b59`) is the reference UI.

## Consequences

- **Plan 1** keeps a simple `sync.sh` on the dev cluster (public fork, no token) and marks it `ponytail:` until Plan 4.
- **Plan 4** owns the webhooks, the onboarding job, the credential helper and the fetch Job.
- **Plan 5** owns Settings → Repos.
- Threat model §7 #30 covers the fetch credential.

## Revisit if

- A repo is too large for a full clone (partial clone, bundle-URIs; ADR 0011's own "revisit if").
- GitLab or Bitbucket arrive (the `Forge` seam, ADR 0049). Their "which repos" source of truth follows the same rule: the forge decides.
