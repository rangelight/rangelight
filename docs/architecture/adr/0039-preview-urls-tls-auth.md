# 0039. Preview URLs, TLS and auth: wildcard hosts with forward-auth to the API

- Status: accepted
- Date: 2026-11-01
- Deciders: the owner
- Closes item 2 of "Must design before code".

## Decision

- **URLs:** `<service>--<session>.preview.<org-domain>`. Host-based, not path-based.
- **TLS:**
  - One wildcard certificate from cert-manager via DNS-01, using the org's DNS-provider credentials. A bring-your-own wildcard certificate also works.
  - On k3d, a local CA issues `*.preview.localtest.me`, so the quickstart works offline.
- **Routing:** the cluster's existing ingress or Gateway API implementation (BYO). The bundled default is k3s's Traefik.
- **Auth:** **forward-auth to the Rangelight `api`** on every request.
  - The `api` checks the OIDC session cookie, which is scoped to the preview domain.
  - It then runs a Cedar check: "may this user view session X?"
  - Every view is audited.
  - No extra auth component.
- **In-namespace traffic** (Playwright, agents) uses service DNS and never passes through the edge.
- **Previews are never public.**
