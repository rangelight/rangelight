# 0006. Extensions: WASM components plus an open, signed catalog (no pod per extension)

- Status: superseded in part by ADR 0017 (2026-10-30): no WASM plugin runtime; the signed catalog is kept for non-executable packs and MCP manifests.
- Date: 2026-10-29
- Deciders: the owner
- Supersedes: the handoff leaning "no WASM in v1" (area M), and the earlier "out-of-process connector = container" idea.

## Context

We need an **open marketplace** for connectors and other extensions.
- A container per connector does not scale. It means N Deployments, each with its own images, CVEs, upgrades and resource requests.
- It also fits badly with a hosted multi-tenant tier.

Prior art studied: Scryer (github.com/scryer-media/scryer, read on 2026-10-29). It uses:
- WASI P2 components under wasmtime, with one WIT world per plugin family
- a host-enforced capability descriptor
- a central catalog, signed with Sigstore keyless and pinned to the signing repo+workflow identity
- delegation to the author's own repo for exactly one plugin
- three tiers: `official`, `verified_community` and `unverified`

## Decision

**The runtime is chosen by extension kind. There is never a pod per extension.**

| Kind | Runtime |
| --- | --- |
| skills, policy packs, preview presets, pipeline templates | data only, loaded into config snapshots (ADR 0005) |
| tracker/forge connectors, field mappers, evidence parsers, policy functions, notifiers | **WASM components** (WASI P2+), one **WIT world per family**, run in-process in a shared sandboxed host |
| gates (tests, scans) | containers run as Jobs inside the session's own pod/namespace |
| MCP servers | remote: through the MCP gateway; stdio: a shared, pooled MCP runner |

**The capability manifest is the sandbox.**
- A component gets no ambient network, filesystem or secrets.
- The host grants only what the manifest declares and Security approved:
  - HTTP to the egress hosts it declared
  - secrets by reference, injected by the host
  - a scoped key-value store
  - a logging API
- Each tenant gets its own instance, with memory, fuel and epoch limits.

**The open catalog, following Scryer's model, adapted:**
- **The index** is a public git repo (Krew-index style).
  - Publishing means a PR adding `{id, family, source repo, signing workflow, OCI ref, SDK/host semver constraints}`.
  - Catalog CI runs the family **conformance suite** and a **capability-diff** check.
  - The merged, signed index is the trust root for `verified_community`.
- **Artifacts** are OCI artifacts in the author's own registry.
  - They are signed with Sigstore keyless.
  - The **signer identity (repo + workflow) is pinned per entry**. An author can publish only their own entries.
  - Each artifact carries an SBOM and SLSA provenance.
- **Tiers:**
  - `official`: Rangelight's own repo and workflow.
  - `verified_community`: in the index and passing conformance.
  - `unverified`: installed manually by OCI ref.
  - Org policy can ban any tier.
- **Install:** an ADR 0005 config PR that pins the digest. The receipt cites the digest and the tier.
- **Mirroring:** orgs mirror the index and the artifacts into their own registry (Zot or Harbor), which gives air-gap support.
- **The Sigstore trust root** is embedded in the release, so verification works offline.
- **Dogfooding:** the first-party Linear and GitHub connectors ship as components through the same catalog.

## Consequences

- One shared host serves thousands of extensions, with microsecond calls and no per-extension ops.
- Authors write in Rust, Go, TS or Python, as far as the guest toolchains allow (being verified).
- We own a versioned WIT surface per family. Breaking it requires major-version discipline and compatibility constraints.
- Reviewing capability diffs becomes a Security workflow in the UI.

## Revisit if

- Go hosting of components turns out immature. Then the host moves to a Rust plugin host or library (feeds into the language ADR), or uses Extism/wazero core-wasm as an interim.
- A family needs long-running or stateful behaviour that doesn't fit components. Then it uses the pooled runner.

## Research notes (2026-10-29), input to the language ADR

- **WASI 0.3** was ratified on 2026-06-11, and 0.3.1 on 2026-08-11. Wasmtime 46 and later implement it. The guest toolchains for it are still in progress. Target **WASI 0.2 worlds now** and add 0.3 worlds side by side later.
- **The Component Model is not a formal standard.** It is Phase 1 in the W3C Community Group, and 1.0 will bring an ABI change. It is used in production by Zed (Wasmtime plus versioned WIT), wasmCloud v2 and Scryer.
- **Go hosting of components is immature:**
  - wazero declined the Component Model ("not planned").
  - wasmtime-go v50's component API is partial, has no confirmed `wasi:http` host, and ships x86_64-only prebuilt binaries.
  - gravity and wacogo are experimental.
- **A Rust wasmtime host is mature:**
  - `wasmtime-wasi-http` provides a `send_request` hook for the egress allowlist.
  - epoch deadlines, the pooling allocator and `ResourceLimiter` cover CPU and memory limits.
  - releases are monthly, with a 24-month LTS.
- **Guests:** Rust is first-class. componentize-go needs Go 1.27.1+. componentize-py exists. ComponentizeJS is experimental and embeds about 8 MB of engine per component.
- **Distribution:** use the CNCF wasm OCI artifact layout and `wkg`. warg is no longer developed.
- **Alternatives rejected:**
  - Extism: a proprietary ABI and lock-in risk for a public marketplace.
  - Nango: ELv2, not open source.
  - Airbyte: Core is ELv2. Its declarative CDK *concepts* (MIT) are borrowable.
