# 0022. Receipt format: an in-toto Statement in DSSE; signed with org keys and WebAuthn

- Status: accepted. The rest of area O (hash chain, anchoring, sealing, retention, SIEM) is still to grill.
- Date: 2026-10-30
- Deciders: the owner

## Decision

- **Format:** an in-toto Statement with predicate type `https://rangelight.dev/receipt/v1` (the domain is frozen by ADR 0051; v1alpha1 until v1, ADR 0027), wrapped in a DSSE envelope.
  - **Subjects:** the PR head commit (`digest: {gitCommit: <sha>}`) and the image digests the session used.
  - **The predicate contains:**
    - criteria, with proof artifact digests
    - per-hunk provenance
    - capabilities granted vs used
    - signed decisions
    - the config snapshot SHA
    - the runtime and sandbox class
    - the event-log hash-chain head
  - The Workbench Receipt tab is a rendering of it.
  - It is verifiable with `cosign verify-attestation`, in-toto tooling and the verify script (ADR 0033; ADR 0044).
- **Signing:**
  - **Machines:** cosign with an org-owned, KMS-backed key (OpenBao Transit, AWS KMS, …). The key never leaves the KMS, and the public key is published in the config repo.
  - **Humans:** a WebAuthn passkey signs each Decision.
  - **`scale` (optional):** a self-hosted Sigstore stack (Fulcio, Rekor, CT log) for keyless identities and a private transparency log.
  - **Public Sigstore:** opt-in only for open-source repos, because public Rekor would leak private metadata.

## Revisit if

- A standard predicate for agent provenance emerges (in-toto or SLSA). Then we align or upstream ours.

## Amendment (external review, 2026-10-31)

- **Verification at launch:** `cosign verify-blob-attestation` plus the published, CI-tested script (ADR 0033). It checks the signature, the predicate, and the session hash chain against the head in the receipt.
- **WebAuthn-signed decisions are deferred.** At launch, a decision is a signed control-plane event naming the authenticated user.
