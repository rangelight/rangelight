# The Rangelight Helm chart

Rangelight on any Kubernetes cluster, in the namespace `rangelight-system` (Pod Security `restricted`).
Every dependency is bundled and swappable for your own (ADR 0009). `values.yaml` documents each key and
`values.schema.json` checks it.

| Component | Bundled (default) | Your own |
| --- | --- | --- |
| Postgres | The CloudNativePG operator (chart `cloudnative-pg`) and one cluster `rangelight-pg`: database `api`, owner `rl_api` | `postgres.enabled=false`, `postgres.external.secretRef` |
| NATS | One NATS server (chart `nats`) with JetStream on a PVC from the default StorageClass | `nats.enabled=false`, `nats.external.url` |
| Git mirror | Off by default: `gitMirror.enabled=true` with `gitMirror.image` (by digest) and `gitMirror.repos` runs the mirror (`services/git-mirror/`) on a PVC | — |

**The runner** (ADR 0018, `services/runner/`) is off by default too: `runner.enabled=true` with `runner.image`
and `runner.agentImage` (by digest) runs it in its own namespace, `runner.namespace` (default `rangelight-runner`).
The cluster owner creates that namespace first (Pod Security `restricted`); the chart never does, so no
`rangelight-system` workload can mount the runner's ServiceAccount. Lanes need Cilium for their Anthropic egress.
The `Session` CRD (`files/crds/session.yaml`) installs and upgrades with the runner, and uninstall keeps it. The runner's ClusterRole holds only what a lane needs, each rule with its reason
(`templates/runner/rbac.yaml`).

## Install

```sh
RANGELIGHT_KUBE_CONTEXT=<context> scripts/cluster.sh up      # installs the chart (two passes, see below)
RANGELIGHT_KUBE_CONTEXT=<context> scripts/cluster.sh smoke   # psql and a JetStream round trip
```

By hand, run the same command twice:
`helm upgrade --install rangelight deployments/helm/rangelight --namespace rangelight-system --wait`.
The first pass installs the operator and its CRDs. The chart renders the Postgres cluster only once its CRD
exists, so the second pass creates it.

## Uninstalling keeps the database

The cluster `rangelight-pg` carries `helm.sh/resource-policy: keep`: `helm uninstall`, `postgres.enabled=false`
or a render without the CloudNativePG CRD never delete it (CloudNativePG would delete its volumes with it).
To drop the database, delete it by hand: `kubectl -n rangelight-system delete cluster rangelight-pg`.
`scripts/cluster.sh down` deletes the whole namespace, and with it the database.

## What the api gets

The ConfigMap `rangelight-api` holds the api's connection settings (ADR 0049 §6):

- `RANGELIGHT_API_POSTGRES_URL`: `secret://rangelight-pg-app#uri` (bundled), the Secret CloudNativePG
  generates, or `secret://<postgres.external.secretRef.name>#<key>`. There's no password in values or git.
- `RANGELIGHT_API_NATS_URL`: `nats://rangelight-nats.rangelight-system.svc:4222`, or `nats.external.url`.

## Network

NetworkPolicies let only pods labelled `app.kubernetes.io/part-of: "rangelight"` in `rangelight-system` reach
Postgres (5432) and NATS (4222), plus the CloudNativePG operator to its instances (5432, 8000). The git mirror
(8080) accepts only namespaces labelled `rangelight.dev/lane: "true"` (the runner labels each lane's) and the
namespace `rangelight-runner`.

## Changing it

- Dependencies are vendored in `charts/` and locked in `Chart.lock`. After a version bump in `Chart.yaml`:
  `helm dependency update deployments/helm/rangelight`.
- Images are pinned by tag and digest. Never bump the Postgres major version in place.
- `scripts/check.sh deploy` runs `helm lint`, kubeconform on both modes, the YAML style on our templates'
  output, and the render tests `test/{template,git-mirror,runner}.sh`.
