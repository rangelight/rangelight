# The Rangelight Helm chart

Rangelight on any Kubernetes cluster, in the namespace `rangelight-system` (Pod Security `restricted`).
Every dependency is bundled and swappable for your own (ADR 0009). `values.yaml` documents each key and
`values.schema.json` checks it.

| Component | Bundled (default) | Your own |
| --- | --- | --- |
| Postgres | The CloudNativePG operator (chart `cloudnative-pg`) and one cluster `rangelight-pg`: database `api`, owner `rl_api` | `postgres.enabled=false`, `postgres.external.secretRef` |
| NATS | One NATS server (chart `nats`) with JetStream on a PVC from the default StorageClass | `nats.enabled=false`, `nats.external.url` |

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
Postgres (5432) and NATS (4222), plus the CloudNativePG operator to its instances (5432, 8000).

## Changing it

- Dependencies are vendored in `charts/` and locked in `Chart.lock`. After a version bump in `Chart.yaml`:
  `helm dependency update deployments/helm/rangelight`.
- Images are pinned by tag and digest. Never bump the Postgres major version in place.
- `scripts/check.sh deploy` runs `helm lint`, kubeconform on both modes, the YAML style on our templates'
  output, and `test/template.sh`.
