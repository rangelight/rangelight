# 0035. Packaging: one image per service

- Status: accepted
- Date: 2026-10-31
- Deciders: the owner

## Decision

- **Build structure, following a previous project:**
  - one Rust workspace
  - a shared `rust-builder` stage that compiles every binary
  - one small Dockerfile per service under `build/package/docker/<service>/`, copying one binary into a date-pinned slim base
  - non-root uid 1000
  - full OCI labels
  - a single `docker-bake.hcl`
- **Images:**

| Image | Language | Runs |
| --- | --- | --- |
| `api`: Connect API, also serves the web app's static assets | Rust | central |
| `admission`: queue, quotas, preemption | Rust | central |
| `relay`: outbox → NATS | Rust | central |
| `checkpoint`: Merkle checkpoints and anchoring | Rust | central |
| `gateway`: LLM, MCP, token broker, web egress | Rust | per cluster |
| `runner` | Go | per cluster |
| `git-mirror`: nginx, git and our hooks | config and scripts | per cluster |
| `agent`: the base image for session pods | TS | per session |

- Every image is signed and has an SBOM and SLSA provenance.
- Services share crates and talk to each other over NATS or Connect, never through a shared database role. Each gets its own `lf_*` role (ADR 0030).

## Revisit if

- The number of images becomes a burden on a single node. Then a combined `all-in-one` image for k3s is an option.

## Amendment (2026-11-06)

- **Rust binaries live under `control-plane/services/<name>`** (ADR 0049 §1). The `rld` sidecar (ADR 0045) is one more small Rust image.
