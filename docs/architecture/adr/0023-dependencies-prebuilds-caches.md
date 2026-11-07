# 0023. Dependencies in seconds: prebuilds, pull-through caches, trusted-write caches

- Status: accepted. The cache products are pending research.
- Date: 2026-10-30
- Deciders: the owner

## Decision

**The start-time budget:**
- claim: under 1s
- image: about 2s (pre-pulled; lazy-pull snapshotter as a fallback)
- worktree: 1–3s (node-local bare mirror with alternates)
- dependencies: 2–5s
- neighbours and migrations: in parallel

**Three layers for dependencies:**
1. **Prebuilt repo images at `main`.**
   - Contents: the toolchain, the dependencies at `main`, and a warm build cache.
   - Keyed by the devcontainer config digest and the lockfile digests (ADR 0013).
   - Rebuilt by a trusted job on each push to `main`, and pre-pulled on nodes.
2. **In-cluster pull-through caches** for session-specific changes: npm, PyPI, Go modules, crates, Maven/Gradle, NuGet and OCI. These are the only egress for packages (ADR 0016).
3. **Content-addressed package managers** (pnpm, uv, the Go module cache, cargo) make installs mostly link operations.

**Compiler caches:**
- sccache (Rust, C/C++) and `GOCACHEPROG` (Go), backed by object storage.
- Turbo, Nx and Bazel remote caches where repos use them.

**The anti-poisoning rule:** shared caches are written only by trusted jobs from `main`. Sessions read the shared layer and write to a private, per-session layer and namespace.

## Consequences

- A session at `main` installs nothing.
- Lockfile drift costs only the delta.
- Prebuild jobs become part of the platform. Their cost and their freshness are visible in the Workbench.

## Revisit if

- The cost of rebuilding prebuilds on busy monorepos is too high. Options then: prebuild per path or package, or debounce rebuilds.
