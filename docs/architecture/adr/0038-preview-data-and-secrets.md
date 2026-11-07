# 0038. Prod-shaped data and secrets for previews

- Status: accepted
- Date: 2026-11-01
- Deciders: the owner
- Closes item 1 of the decisions log's "Must design before code".

## Decision

**Each data store declares its mode in `rangelight.yaml`.**

- **`fixtures`** (default): migrations plus a seed command owned by the repo. Deterministic, with no PII.
- **`masked-snapshot`:**
  - A trusted nightly job builds a **golden copy** from staging or prod with **Greenmask** (Apache-2.0).
  - The masking rules are code, kept in `rangelight-config` and reviewed by Security through CODEOWNERS.
  - Each session gets a **copy-on-write clone**: a CSI VolumeSnapshot clone where the driver supports it (TopoLVM thin, EBS), otherwise a restore. This is the accelerator role in ADR 0010.
  - Unmasked data never enters a session namespace.
  - The golden copy's digest goes into the receipt.
- **`shared-baseline`** (request routing to a shared heavy dependency) comes after launch.

**External dependencies:** each one is set to one of:
- `mock`: WireMock or Prism (Apache-2.0), driven by an OpenAPI spec.
- `sandbox`: the vendor's test mode with preview-scoped credentials.
- `block`: unreachable, and the receipt records it.

**Secrets:**
- Each profile has a **preview secret set** of low-privilege, preview-only credentials, such as test keys and a throwaway DB user per session.
- Rangelight materialises it into the session namespace.
- **Prod secrets never enter a preview.** This is enforced by Cedar policy and the credential-reference interface.

**Cluster-scoped resources:** CRDs, operators and other cluster-scoped objects in an org's charts must be marked **shared and pre-installed** in the profile. Rangelight refuses to render them per session.

## Consequences

- The golden-copy job is a privileged, trusted component with access to the staging or prod DB. It runs outside session namespaces and gets its own threat-model entry.
- Clone speed depends on the storage driver. The Workbench shows which mode each session used and how long it took.

## Revisit if

- Logical masking is too slow for large databases. Then consider physical masking or database branching (Neon is Apache-2.0 but heavy).

## Amendment (second external review, 2026-11-02)

- **Previews and preview data move to v0.2.0, the public alpha** (ADR 0044, item 1).
- **Fixtures first** (owner, ADR 0044 item 2). v0.2.0 supports `fixtures` and the `mock`/`sandbox`/`block` modes for external deps.
- `masked-snapshot` is the first capability after v0.2.0, once its full lifecycle is designed and tested (build, fail-closed, PII scan, clone, reset, migrate, destroy, restore, failure UX).
- The v1alpha1 `api` rejects `masked-snapshot` with "not supported in this release".
- **Secret materialisation** gets a concrete design in the first task of Plan 9 (v0.2, previews and preview data): who creates each preview secret, where it's stored, rotation, and destruction with the namespace. Saying "Cedar enforces it" is not enough.
