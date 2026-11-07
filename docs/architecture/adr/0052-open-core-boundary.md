# 0052. The open-core boundary: Apache-2.0 forever; the paid tier is hosting, in a separate repo

- Status: accepted
- Date: 2026-11-06
- Deciders: the owner
- Amends: ADR 0026 (repos), ADR 0004 (hosted tier stays BYOK), ADR 0051 (trademark before the hosted tier)
- Evidence: research on Prometheus, Grafana, Traefik, Tokio, Zed, Bytebase, GitLab, Sentry, PostHog, Temporal, Supabase, Cal.com, HashiCorp and Elastic (2026-11-06, kept with the owner's notes, outside this repo)

## Context

Every large fork in the studied projects followed a company relicensing an established project to non-OSI terms: Terraform and Vault to BSL (2023, OpenTofu and OpenBao), Elasticsearch to SSPL (2021, OpenSearch). Each relicence was possible because a CLA gave the company the copyright. Cal.com ran AGPL plus a proprietary `/ee` folder for five years and closed its source in April 2026. Projects that gated basic security (Elastic, TLS and RBAC) later moved it back to free.

The projects that monetise without any of this (Temporal, Supabase) sell **running** the open code: tenancy, backups, retention, operations. Rangelight's plan, a small bring-your-own-key hosted tier that never resells inference (ADR 0004), is that model. ADR 0003 already puts `org_id` and row-level security in the first migration, and ADR 0028 makes authorisation Cedar policy, so the hosted tier needs no refactor of the core.

## Decision

1. **Apache-2.0 forever.** Code in `rangelight/rangelight` is never relicensed. `GOVERNANCE.md` states it as a promise.
2. **DCO, no CLA.** Contributors keep their copyright, which makes the promise credible: nobody, including the owner, can relicense contributed code.
3. **The paid tier is hosting**, built in a private `rangelight/cloud` repo that deploys the public images and chart by digest. The public repo has no `ee/` directory, no licence key check and no feature gate.
4. **Free forever:** everything in the public repo, explicitly including receipt signing and verification, TLS, single-cluster RBAC, the lane isolation model and the observed gate. No feature ever moves from free to paid.
5. **If a self-hosted enterprise offering ever exists,** its line follows GitLab's buyer rule (the feature goes to the tier of whoever cares most about it). The candidates are SSO/SAML/SCIM, cross-cluster fleet views, compliance evidence packs built from receipts, and support. They ship as a separate image that implements a public seam, never as gated code in the public repo.
6. **The seams the hosted tier needs** are scheduled, so ADR 0049 §2.2 allows them: an org provisioning and lifecycle hook, and a receipt and event export sink (ARCHITECTURE.md §10).
7. **No telemetry in cluster components.** Nothing Rangelight installs in a cluster phones home. Usage numbers come from the hosted tier's own operations only. `TELEMETRY.md` says so.
8. **AI contributions are governed by provenance, not banned.** `AI_POLICY.md`: AI-assisted work is welcome with disclosure, a linked accepted issue and an accountable human who signs off (DCO); a `Rangelight receipt` from a lane is accepted as provenance. Agent identity and commit rules are ADR 0050's 2026-11-06 amendment.
9. **The trademark protects the hosted tier** (ADR 0051): registration and a usage policy come before the hosted tier launches.

## Consequences

- Day-zero files: `LICENSE`, `NOTICE`, `GOVERNANCE.md` (promise, free/paid line), `AI_POLICY.md`, `TELEMETRY.md`.
- The hosted tier's security issues need their own disclosure scope in `SECURITY.md` when it launches.
- Revenue can only come from running Rangelight well, and from support. That limits upside; it's the price of the trust a receipts product needs.

## Revisit if

- A cloud provider resells Rangelight as a service and takes the hosted tier's market. The answer is the trademark and the quality of the hosted tier, never relicensing the core.

## Amendment (2026-11-06)

- **The public verifier page's bare counter isn't telemetry** (ADR 0056 §2): nothing in a cluster sends it, and it stores no IP, repo or receipt content.

## Amendment (2026-11-06, every seat)

- **Also free forever** (ADR 0057): unlimited viewer seats, the Security view, audit search and SIEM export, and policy dry-run.
