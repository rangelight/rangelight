# 0013. Session toolchains: devcontainer.json first

- Status: accepted
- Date: 2026-10-30
- Deciders: the owner

## Decision

- **Image source:**
  - Use the repo's `devcontainer.json` (image plus Features) when it is present.
  - Otherwise use a Rangelight base image for the detected stack.
  - Org policy can require approved base images.
- **Build and cache:**
  - Images are built with BuildKit.
  - They are cached in the registry (Zot/Harbor), keyed by the digest of the devcontainer config.
  - Warm starts never build.
  - The receipt records each image digest.
- **Cross-repo sessions:** one container per repo in the same agent pod, all sharing the workspace volume.

## Consequences

- The OTel Demo (about ten languages) exercises this early.
- We ship and maintain a small set of base images, built and signed through our own supply-chain pipeline.

## Revisit if

- Reproducibility demands Nix, or devcontainer Features prove too slow or flaky to build at scale.

## Amendment (2026-10-31): Jenkins-style toolchain sidecars rejected

**Considered:** a `podTemplate`-style container per toolchain. **Rejected:**
- The agent would need either Kubernetes `exec` access (a token in the pod breaks ADRs 0004 and 0015) or a shim plus PATH wrappers.
- Mixed-tool builds become awkward.

**Kept:** the composability of upstream toolchains, through devcontainer **Features** merged at build time into **one image per repo**, with dependencies pre-baked (ADR 0023).

- **At launch:** one container per session.
- **Cross-repo sessions (later):** one container per repo, joined by an in-pod exec shim over a shared socket. No Kubernetes API is involved.
