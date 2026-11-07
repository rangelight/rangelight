# 0010. Sessions are rebuildable ("cattle"); snapshots are only an accelerator; fork/rewind is in M1

- Status: accepted
- Date: 2026-10-29
- Deciders: the owner

## Context

Hibernate, preemption, fork and data cloning were going to depend on CSI VolumeSnapshots. Not every self-hosted cluster has them. The owner asked why we need snapshots at all.

## Decision

- **Every session can be rebuilt from:** git checkpoints + data setup as code + the event log.
  - **Code:**
    - The agent auto-commits each step as a checkpoint to hidden refs on the in-cluster mirror: `refs/rangelight/sessions/<id>/cp/<n>`.
    - Uncommitted work is committed before any stop.
  - **Agent state:** the event log in Postgres, plus transcripts in object storage.
  - **Data:** the repos' own migrations as code. A dedicated seeding/fixtures strategy beyond migrations is **deferred**, by the owner's call.
  - **Build output and dependencies:** the build caches (area Q).
- **Hibernate** deletes the pods. Keeping the PVC is an optional optimisation. Wake rebuilds the session.
- **Node loss** is a rebuild, not an incident.
- **CSI VolumeSnapshots and clones** are used as an **accelerator** when the cluster offers them, for example to clone a large database fast. Correctness never depends on them.
- **Fork/rewind moves into M1:** branch from checkpoint *n*, then replay the event log up to *n*, in a new session. Receipts are compared side by side.

## Consequences

- No storage-system requirement on any cluster (ADR 0009).
- Receipt "Replay" literally means a rebuild from the same inputs.
- Wake time depends on the caches. The UI must show progress honestly.
- Checkpoint refs need a GC policy, tied to session close and receipt retention.

## Revisit if

- Rebuild times for real orgs make wake or preemption painful. Then we invest in the snapshot accelerator per platform.

## Amendment (external review, 2026-10-31)

- **"Replay" means rebuilding the *environment* from the same inputs:** commits, image digests, migrations and the event log up to *n*.
- It **does not** promise that the LLM will reproduce the same output. The UI and the receipt must say this plainly.
- **Seeding is the open design problem** for prod-shaped environments. It must be designed before v0.2 (see "Must design before code" in the log).

## Amendment (2026-11-06)

- **Hibernate is now "close"** (ADR 0045 §4): a WIP commit is pushed and the diff and transcript are snapshotted before the pod goes; the lane reopens from its branch and resumes the same agent session.
