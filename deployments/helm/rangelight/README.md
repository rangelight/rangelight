# The Rangelight Helm chart

Rangelight on any Kubernetes cluster, in the namespace `rangelight-system` (Pod Security `restricted`).
Every dependency is bundled and swappable for your own (ADR 0009). `values.yaml` documents each key and
`values.schema.json` checks it.

| Component | Bundled (default) | Your own |
| --- | --- | --- |
| Postgres | The CloudNativePG operator (chart `cloudnative-pg`) and one cluster `rangelight-pg`: database `api`, owner `rl_api` | `postgres.enabled=false`, `postgres.external.secretRef` |
| NATS | One NATS server (chart `nats`) with JetStream on a PVC from the default StorageClass; every client signs in with its own identity (see NATS sign-in) | `nats.enabled=false`, `nats.external.url`, `nats.auth.existingSecret` |
| Git mirror | Off by default: `gitMirror.enabled=true` with `gitMirror.image` (by digest) and `gitMirror.repos` runs the mirror (`services/git-mirror/`) on a PVC | — |
| Sign-in (OIDC) | Dex (chart `dex`): GitHub, limited to the members of `dex.github.org` | `dex.enabled=false`, `oidc.issuer`, `oidc.clientSecretRef`, `oidc.memberClaim` |

**The product** is off by default, since its images are pinned per install: `api.enabled` (with `api.image`,
`linear.team` and `linear.apiKeySecretRef`), `relay.enabled` (with `relay.image`) and `web.enabled` (with
`web.image`) run the api, the relay and the Workbench, one replica each (the relay is the outbox's single
writer). `ingress.enabled` renders one Ingress for the host of `publicUrl`; it needs `ingress.from` and installs
no controller (see Ingress).

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
- With the api: the Secret named by `linear.apiKeySecretRef` (key `api-key` by default) holding a Linear API key
  that reads `linear.team`'s issues.

```sh
RANGELIGHT_KUBE_CONTEXT=<context> RANGELIGHT_VALUES=<your values.yaml> scripts/cluster.sh up   # two passes, see below
RANGELIGHT_KUBE_CONTEXT=<context> scripts/cluster.sh smoke   # psql, and NATS refuses an anonymous client
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

The ConfigMap `rangelight-api` holds the api's settings (`configs/api.default.toml`, ADR 0049 §6). The Secrets
they name are mounted into the api's pod as files, only the keys named, under `/var/run/rangelight/secrets/<name>/<key>`;
the relay gets the database's the same way. No secret is ever an environment value.

- `RANGELIGHT_API_DATABASE_URL`: `secret://rangelight-pg-app#uri` (bundled), the Secret CloudNativePG
  generates, or `secret://<postgres.external.secretRef.name>#<key>`. There's no password in values or git.
- `RANGELIGHT_API_NATS_URL`: `nats://rangelight-nats.rangelight-system.svc:4222`, or `nats.external.url`.
- `RANGELIGHT_API_PUBLIC_URL`, `RANGELIGHT_API_OIDC_ISSUER` (`<publicUrl>/dex`, or `oidc.issuer`),
  `RANGELIGHT_API_OIDC_CLIENT_ID`, `RANGELIGHT_API_OIDC_MEMBER_CLAIM`, and `RANGELIGHT_API_OIDC_CLIENT_SECRET` as a reference to
  `oidc.clientSecretRef`.
- `RANGELIGHT_API_LINEAR_TEAM` and `RANGELIGHT_API_LINEAR_API_KEY` (a reference to `linear.apiKeySecretRef`).
- `RANGELIGHT_API_EXTRA_CA_FILE`, only with `tls.extraCaSecretRef.name`: that Secret's key, mounted like the
  others (see Sign-in).
- `RANGELIGHT_API_REPO`, `_BASE_REF`, `_MEMBER`, `_STREAM_REPLICAS` from `api.repo` (required), `api.baseRef`,
  `api.member` (required) and `api.streamReplicas`: where the sessions work, and for whom.
- `/etc/rangelight/api.toml` (the ConfigMap `rangelight-api-file`): the NATS auth callout's `[nats_auth]`, with
  this cluster's issuer (`nats.auth.issuer`), the runner's ServiceAccount, the accounts, and its seeds as
  references to `rangelight-api-nats` (or `nats.auth.existingSecret`). Its ServiceAccount `rangelight-api` holds
  only `system:service-account-issuer-discovery`; the pod mounts its NATS token and, to fetch the cluster's
  signing keys, a token for the API server and the cluster CA.

## NATS sign-in

Every NATS client signs in as itself (ADR 0021 amendment). The api, the relay, the runner and each lane present
their projected ServiceAccount token (audience `rangelight-nats`, an hour, mounted at
`/var/run/secrets/rangelight/nats/token` and read again at every connect). The api is the server's auth
callout: it checks the token against this cluster's issuer (`nats.auth.issuer`, keys from
`https://kubernetes.default.svc/openid/v1/jwks`), maps it to one identity and answers with a user JWT holding only
that identity's subjects (`test/nats-permissions.json`) for at most an hour. Anything else is refused, anonymous
clients included. Find your cluster's issuer with `kubectl get --raw /.well-known/openid-configuration`.

- **Keys.** A `pre-install`/`pre-upgrade` hook Job (`files/nats/keys.sh`, nats-box) creates them once:
  `rangelight-api-nats` (the callout issuer's seed and the api's auth-user seed, mounted by the api) and
  `rangelight-nats-auth` (their public keys, read by the server). Neither is in values, and uninstall keeps both.
  With GitOps or `helm template`, create one Secret yourself and name it in `nats.auth.existingSecret`: keys
  `issuer-seed` (`nk -gen account`), `auth-user-seed` (`nk -gen user`), and for the bundled server `issuer` and
  `auth-user`, their public keys (`nk -inkey <seed file> -pubout`). The Job doesn't run then.
- **Rotation.** Delete both Secrets, upgrade (the Job makes new keys), then restart NATS and the api:
  `kubectl -n rangelight-system delete secret rangelight-api-nats rangelight-nats-auth`, `helm upgrade …`,
  `kubectl -n rangelight-system rollout restart statefulset rangelight-nats deployment rangelight-api`. Clients
  sign in again on their own. ServiceAccount tokens rotate by themselves: the kubelet renews them.
- **Your own NATS** (`nats.enabled=false`) runs in operator mode, the callout on account RL and the lanes in an
  account of their own (`files/nats/auth.conf` is the same layout in config mode). With `nsc`:
  1. `nsc add account RL`, `nsc edit account RL --js-enable 1 --sk generate`: that signing key's seed is
     `issuer-seed`. `nsc add account LANE`, `nsc edit account LANE --conns 1024 --subscriptions 1 --payload 131072
     --sk generate`: its seed is `lane-issuer-seed`.
  2. The terminals between them: `nsc add export --account RL --subject 'rl.*.pty.*.in'` and
     `nsc add import --account LANE --src-account RL --remote-subject 'rl.*.pty.*.in'`; the same from LANE to RL
     for `rl.*.pty.*.out`.
  3. The lane check: `nsc add export --account SYS --service --subject '$SYS.REQ.ACCOUNT.<LANE>.CONNZ'` and
     `nsc add import --account RL --service --src-account SYS --remote-subject '$SYS.REQ.ACCOUNT.<LANE>.CONNZ'`,
     where `<LANE>` is LANE's public key.
  4. The api's own user: `nsc add user -a RL rangelight-api --allow-sub '$SYS.REQ.USER.AUTH,_INBOX_callout.>'
     --allow-pub '$SYS.REQ.ACCOUNT.<LANE>.CONNZ' --allow-pub-response`; its `.creds` file is `auth-user-seed`.
  5. `nsc edit authcallout --account RL --auth-user <that user's public key> --allowed-account <LANE>`, then
     `nsc push -A`. Set the server's `max_payload` to 131072.
  6. Values: `nats.external.url` (`tls://`, or `nats://` to a Service in this cluster), `nats.auth.account` and
     `nats.auth.laneAccount` (the two public keys), and `nats.auth.existingSecret` holding `issuer-seed`,
     `lane-issuer-seed` and `auth-user-seed`.
- **Upgrading from before sign-in** moves JetStream out of NATS's global account into `RL`. The old streams stay
  on the volume, unused. At every start the relay puts back each session's last published desired state from
  Postgres wherever the desired stream lacks it, before it publishes anything newer, so runners lose nothing.
  Status reports the api hadn't recorded yet are lost; each runner reports again at a session's next phase
  change. A changed `auth.conf` reaches the server at its next restart: `kubectl -n rangelight-system rollout
  restart statefulset rangelight-nats`.
- **Close every live session before upgrading to this version.** The runner now marks each lane namespace with
  its Session's UID (`rangelight.dev/session-uid`) and never adopts a namespace without that mark, so nobody can
  make one in advance and take over its lane. Namespaces from earlier versions lack it: after the upgrade their
  sessions fail with `namespace_taken`, and the runner leaves those namespaces, and their pods, in place. Close
  the sessions first (their namespaces go with them), or afterwards delete the leftover namespaces by hand:
  `kubectl get namespaces -l rangelight.dev/lane=true`.

## Sign-in

The api signs browsers in with OIDC (authorization code, PKCE): at bundled Dex, the issuer
`<publicUrl>/dex`, or at your own provider (`dex.enabled=false`, `oidc.issuer`, and a client registered for
`<publicUrl>/auth/callback` whose secret is in `oidc.clientSecretRef`). The Ingress routes `<publicUrl>/dex` to
the Service `rangelight-dex` (port 5556). Dex is one server with in-memory storage: it only brokers sign-ins, and logins
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
- **An issuer behind your own CA.** The api fetches the issuer over HTTPS: with bundled Dex through the
  Ingress at `<publicUrl>/dex`, so with whatever certificate serves `publicUrl`. If that certificate (or your own
  provider's, or a proxy's in front of Linear) comes from your own CA, the api refuses it and sign-in answers 503.
  Put the CA's PEM bundle in a Secret in `rangelight-system` and name it in `tls.extraCaSecretRef` (key `ca.crt`
  by default): the api then trusts the platform's roots plus every certificate in it, for the issuer and Linear.
  A bundle without a certificate, or with one that doesn't parse, stops the api at start.
- **Dex is served on the Workbench's origin** (`<publicUrl>/dex`): one host, one certificate, and the
  issuer is where the browser already is. The trade-off: Dex's pages share the origin with the Workbench,
  so a flaw in them would run with the Workbench's origin (the login cookie stays HttpOnly). Serving Dex on
  its own host is a later option.

## Ingress

`ingress.enabled` renders one `networking.k8s.io/v1` Ingress `rangelight` for the host of `publicUrl`, with
`ingress.className` and `ingress.tls.secretName` if set, and `ingress.annotations` as given (the chart sets no
controller-specific annotation; the terminal WebSocket and `WatchSession` are long-lived, so raise your
controller's read timeout there). Paths, all `Prefix`: each Connect service (`/rangelight.auth.v1alpha1.AuthService`,
`/rangelight.session.v1alpha1.SessionService`), `/auth` and `/v1alpha1/sessions` (the terminal WebSocket) to
`rangelight-api:8080`; `/dex` to Dex; everything else to the Workbench, `rangelight-web:8080`. `ingress.from`
names your controller's pods, by their namespace's labels (required) and their own labels, for example
`--set 'ingress.from.namespaceLabels.kubernetes\.io/metadata\.name=ingress-nginx'`, and
`ingress.from.httpsPort` (required) is their HTTPS container port: 443 for ingress-nginx, 8443 for Traefik's
websecure.

## Network

NetworkPolicies let only pods labelled `app.kubernetes.io/part-of: "rangelight"` in `rangelight-system` reach
Postgres (5432), plus the CloudNativePG operator to its instances (5432, 8000). NATS (4222) admits only its
clients: the api and the relay, and the runner from its own namespace. The git mirror
(8080) accepts only namespaces labelled `rangelight.dev/lane: "true"` (the runner labels each lane's) and the
namespace `rangelight-runner`. Dex takes connections on 5556 only, from `ingress.from` (your ingress
controller); it calls out only to the cluster DNS (`clusterDns`, shared with the lanes) and to
HTTPS outside `clusterCidrs` (the cluster's pod and service CIDRs, k3s's and RKE2's by default). The api takes
connections on 8080 only from `ingress.from`; it calls out to the cluster DNS, Postgres, NATS, `ingress.from`
on `ingress.from.httpsPort` (bundled Dex's issuer, `<publicUrl>/dex`) and HTTPS outside `clusterCidrs` (Linear,
your own OIDC provider, on 443 and on the port of `oidc.issuer` if it names one), and the API server on 443 and
6443, for the cluster's token signing keys: with Cilium by its `kube-apiserver` entity (a CiliumNetworkPolicy),
without it only at `nats.auth.apiServerCidrs`, which you then set (`kubectl get endpointslices -n default -l
kubernetes.io/service-name=kubernetes`). The relay
takes no connections and calls out to the cluster DNS, Postgres and NATS only. The
Workbench takes connections on 8080 only from `ingress.from` and calls out to nothing, DNS included. With your
own Postgres or NATS, the api and the relay may reach port 5432, or the port of `nats.external.url`, at any
address.

## Changing it

- Dependencies are vendored in `charts/` and locked in `Chart.lock`. After a version bump in `Chart.yaml`:
  `helm dependency update deployments/helm/rangelight`.
- Images are pinned by tag and digest. Never bump the Postgres major version in place.
- `scripts/check.sh deploy` runs `helm lint`, kubeconform on both modes, the YAML style on our templates'
  output, and the render tests `test/{template,git-mirror,runner,dex,api,relay,web,ingress,nats}.sh`.
