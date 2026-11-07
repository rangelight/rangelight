# 0024. Progressive readiness; one prebuild artifact (with git objects at main)

- Status: accepted
- Date: 2026-10-30
- Deciders: the owner. The prebuild contents are my call, forced by ADR 0015.

## Decision

- **Progressive readiness.** A session's readiness is a series of layers:
  1. code ready (target ~3s)
  2. dependencies ready
  3. neighbours healthy
  4. schema migrated
  5. service hot
  6. preview reachable

  Each agent tool declares the layer it needs. Reading, planning and editing start at layer 1. Running the service, tests and Playwright wait for their gates.
  - The Workbench shows each layer filling in, with real timings.
  - The receipt records the timings.
  - Headline metric: **time to first agent action**. Secondary metric: time to full environment.
- **One prebuild artifact per repo at `main`.** It contains:
  - the toolchain
  - the dependencies
  - **the git objects (packfile)**
  - the warm build cache

  Worktrees use git alternates pointing at the prebuild's objects and fetch only the delta commits from the mirror.
  - No `hostPath` and no node-local git cache, because Pod Security `restricted` forbids `hostPath` (ADR 0015).
  - Mounting through the image itself, or through Kubernetes image volumes where the cluster supports them, is the builder's call.

## Revisit if

- Prebuild images get too large for big monorepos. Options then: split the git objects into a separate OCI artifact, or use partial clone.
