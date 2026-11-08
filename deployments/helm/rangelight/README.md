# The Rangelight Helm chart

Rangelight on any Kubernetes cluster, in the namespace `rangelight-system` (Pod Security `restricted`).
Every dependency is bundled and swappable for your own (ADR 0009). `values.yaml` documents each key and
`values.schema.json` checks it.

| Component | Bundled (default) | Your own |
| --- | --- | --- |
| Postgres | The CloudNativePG operator (chart `cloudnative-pg`) and one cluster `rangelight-pg`: database `api`, owner `rl_api` | `postgres.enabled=false`, `postgres.external.secretRef` |
| NATS | One NATS server (chart `nats`) with JetStream on a PVC from the default StorageClass | `nats.enabled=false`, `nats.external.url` |
| Git mirror | Off by default: `gitMirror.enabled=true` with `gitMirror.image` (by digest) and `gitMirror.repos` runs the mirror (`services/git-mirror/`) on a PVC | — |
| Sign-in (OIDC) | Dex (chart `dex`): GitHub, limited to the members of `dex.github.org` | `dex.enabled=false`, `oidc.issuer`, `oidc.clientSecretRef`, `oidc.memberClaim` |

**The runner** (ADR 0018, `services/runner/`) is off by default too: `runner.enabled=true` with `runner.image`
and `runner.agentImage` (by digest) runs it in its own namespace, `runner.namespace` (default `rangelight-runner`).
The cluster owner creates that namespace first (Pod Security `restricted`); the chart never does, so no
`rangelight-system` workload can mount the runner's ServiceAccount. Lanes need Cilium for their Anthropic egress.
The `Session` CRD (`files/crds/session.yaml`) installs and upgrades with the runner, and uninstall keeps it. The runner's ClusterRole holds only what a lane needs, each rule with its reason
(`templates/runner/rbac.yaml`).

## Install

`publicUrl` (where browsers reach Rangelight, `https://<host>[:<port>]`) is required, and so is
`dex.github.org` while Dex is bundled. First create the sign-in Secrets in `rangelight-system`:

- `rangelight-dex-github`, keys `client-id` and `client-secret`: a GitHub OAuth app whose callback URL is
  `<publicUrl>/dex/callback`.
- `rangelight-oidc`, key `client-secret`: at least 32 bytes of randomness, for example
  `openssl rand -base64 48`. Dex registers the api as the client `rangelight-api` with it, and the api reads
  it and derives the sign-in cookie's key from it; the api refuses to start with a shorter one.

```sh
RANGELIGHT_KUBE_CONTEXT=<context> RANGELIGHT_VALUES=<your values.yaml> scripts/cluster.sh up   # two passes, see below
RANGELIGHT_KUBE_CONTEXT=<context> scripts/cluster.sh smoke   # psql and a JetStream round trip
```

By hand, run the same command twice:
`helm upgrade --install rangelight deployments/helm/rangelight --namespace rangelight-system --values <your values.yaml> --wait`.
The first pass installs the operator and its CRDs. The chart renders the Postgres cluster only once its CRD
exists, so the second pass creates it.

## Uninstalling keeps the database

The cluster `rangelight-pg` carries `helm.sh/resource-policy: keep`: `helm uninstall`, `postgres.enabled=false`
or a render without the CloudNativePG CRD never delete it (CloudNativePG would delete its volumes with it).
To drop the database, delete it by hand: `kubectl -n rangelight-system delete cluster rangelight-pg`.
`scripts/cluster.sh down` deletes the whole namespace, and with it the database.

## What the api gets

The ConfigMap `rangelight-api` holds the api's settings (`configs/api.default.toml`, ADR 0049 §6):

- `RANGELIGHT_API_DATABASE_URL`: `secret://rangelight-pg-app#uri` (bundled), the Secret CloudNativePG
  generates, or `secret://<postgres.external.secretRef.name>#<key>`. There's no password in values or git.
- `RANGELIGHT_API_NATS_URL`: `nats://rangelight-nats.rangelight-system.svc:4222`, or `nats.external.url`.
- `RANGELIGHT_API_PUBLIC_URL`, `RANGELIGHT_API_OIDC_ISSUER` (`<publicUrl>/dex`, or `oidc.issuer`),
  `RANGELIGHT_API_OIDC_CLIENT_ID`, `RANGELIGHT_API_OIDC_MEMBER_CLAIM`, and `RANGELIGHT_API_OIDC_CLIENT_SECRET` as a reference to
  `oidc.clientSecretRef`.
- `RANGELIGHT_API_REPO`, `_BASE_REF`, `_MEMBER`, `_STREAM_REPLICAS` from `api.repo` (required), `api.baseRef`,
  `api.member` (required) and `api.streamReplicas`: where the sessions work, and for whom.

## Sign-in

The api signs browsers in with OIDC (authorization code, PKCE): at bundled Dex, the issuer
`<publicUrl>/dex`, or at your own provider (`dex.enabled=false`, `oidc.issuer`, and a client registered for
`<publicUrl>/auth/callback` whose secret is in `oidc.clientSecretRef`). Route `<publicUrl>/dex` to the Service
`rangelight-dex` (port 5556), and let your ingress controller through Dex's NetworkPolicy with
`dex.ingressFrom` (its namespace's and pods' labels; by default only the api may reach Dex), for example
`--set 'dex.ingressFrom.namespaceLabels.kubernetes\.io/metadata\.name=ingress-nginx'`. Dex is one server with in-memory storage: it only brokers sign-ins, and logins
live in Postgres, so a restart costs only the sign-ins in flight. Dex reads its config (`templates/dex/config.yaml`)
at start: after changing `publicUrl`, `dex.github.org` or `oidc.clientId`, run
`kubectl -n rangelight-system rollout restart deployment rangelight-dex`.

- **Removing someone from `dex.github.org` takes effect at their next sign-in.** Dex checks the org at
  sign-in only, so a login already open lasts until it has been unused for 8 hours or is 7 days old. To end
  it now, delete the member's logins (their open streams end within `login_recheck_seconds`, 60 s):
  `kubectl -n rangelight-system exec rangelight-pg-1 -- psql -d api -c "DELETE FROM logins WHERE member = '<member>'"`
  (with your own Postgres, run the `DELETE` against the api's database).
- **Your own provider must decide who may sign in.** Rangelight lets in anyone the issuer gives a token
  for `oidc.clientId`: restrict the client to your users (a group, a tenant, an org) at the provider. A
  public multi-tenant provider with an unrestricted client lets anyone in.
- **The member claim.** `oidc.memberClaim` names the member (also the sessions' `member` label). With
  Dex it is `preferred_username`, the GitHub login. With your own provider it must be a claim the provider
  sets and keeps unique, never one users can edit: the first account to present a name owns it, so an
  editable claim lets someone take a colleague's name before they sign in.
- **Changing `publicUrl` changes Dex's issuer** (`<publicUrl>/dex`), and every member is bound to the
  issuer and subject that first signed in as it, so every existing member is refused afterwards. Dex's
  subjects don't change with the URL; rebind them in Postgres before anyone signs in again:
  `UPDATE members SET issuer = '<new publicUrl>/dex' WHERE issuer = '<old publicUrl>/dex';`
- **Dex is served on the Workbench's origin** (`<publicUrl>/dex`): one host, one certificate, and the
  issuer is where the browser already is. The trade-off: Dex's pages share the origin with the Workbench,
  so a flaw in them would run with the Workbench's origin (the login cookie stays HttpOnly). Serving Dex on
  its own host is a later option.

## Network

NetworkPolicies let only pods labelled `app.kubernetes.io/part-of: "rangelight"` in `rangelight-system` reach
Postgres (5432), plus the CloudNativePG operator to its instances (5432, 8000). NATS (4222) admits only its
clients: the api and the relay, and the runner from its own namespace. The git mirror
(8080) accepts only namespaces labelled `rangelight.dev/lane: "true"` (the runner labels each lane's) and the
namespace `rangelight-runner`. Dex takes connections on 5556 only, from the api and from `dex.ingressFrom`
(your ingress controller); it calls out only to the cluster DNS (`clusterDns`, shared with the lanes) and to
HTTPS outside `dex.clusterCidrs` (the cluster's pod and service CIDRs, k3s's by default).

## Changing it

- Dependencies are vendored in `charts/` and locked in `Chart.lock`. After a version bump in `Chart.yaml`:
  `helm dependency update deployments/helm/rangelight`.
- Images are pinned by tag and digest. Never bump the Postgres major version in place.
- `scripts/check.sh deploy` runs `helm lint`, kubeconform on both modes, the YAML style on our templates'
  output, and the render tests `test/{template,git-mirror,runner,dex}.sh`.
