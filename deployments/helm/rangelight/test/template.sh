#!/usr/bin/env bash
# What the chart renders for Postgres and NATS, and the values it refuses. Run by `scripts/check.sh deploy`.
# Usage: deployments/helm/rangelight/test/template.sh [test_name...]   (no argument runs every test)
set -euo pipefail
# shellcheck source=deployments/helm/rangelight/test/lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

EXTERNAL=(--set postgres.enabled=false --set postgres.external.secretRef.name=pg-api
  --set nats.enabled=false --set nats.external.url=tls://nats.example.com:4222)

# --- AC1: CloudNativePG operator and the cluster rangelight-pg ------------------

test_bundled_installs_operator_and_crds() {
  render --show-only charts/cloudnative-pg/templates/deployment.yaml --show-only charts/cloudnative-pg/templates/crds/crds.yaml
  expect_ok
  expect_line "kind: CustomResourceDefinition"
  expect_line "  name: clusters.postgresql.cnpg.io"
  expect_line "        image: \"ghcr.io/cloudnative-pg/cloudnative-pg:1.30.1@sha256:923c267ec29636db3bee20f993d0ec4973fa22998e1adad37da79e4d32b5bc07\""
  expect_restricted
}

test_bundled_cluster_bootstraps_api_owned_by_rl_api() {
  render --api-versions "${CNPG_API}" --show-only templates/postgres/cluster.yaml
  expect_ok
  expect_line '  name: "rangelight-pg"'
  expect_line '  namespace: "rangelight-system"'
  expect_line '      database: "api"'
  expect_line '      owner: "rl_api"'
  expect_line '  enableSuperuserAccess: false'
  expect_line '    helm.sh/resource-policy: "keep"'
  expect_line '      memory: "1Gi"'
}

test_cluster_waits_for_the_operator_crd() {
  render --show-only templates/postgres/cluster.yaml
  expect_refused "could not find template"
}

test_no_credentials_in_values_or_render() {
  render --api-versions "${CNPG_API}"
  expect_ok
  expect_no_match '^kind: "?Secret"?$'
  OUT="$(<"${CHART}/values.yaml")"
  expect_no_match '^ *[A-Za-z]*([Pp]ass(word)?|[Ss]ecret[Vv]alue)[A-Za-z]*:'
}

# --- AC2: NATS with JetStream, one server, file storage on a default-class PVC ---

test_bundled_nats_single_server_with_jetstream_on_a_pvc() {
  render --show-only charts/nats/templates/stateful-set.yaml
  expect_ok
  expect_line "  replicas: 1"
  expect_line "  volumeClaimTemplates:"
  expect_line "          storage: 10Gi"
  expect_no_match 'storageClassName'
  expect_no_match 'image: .*:latest'
  expect_restricted
  render --show-only charts/nats/templates/config-map.yaml
  expect_ok
  grep -qE -e '"jetstream": \{' <<<"${OUT}" || fail "JetStream is not enabled"
  grep -qE -e '"store_dir": "/data"' <<<"${OUT}" || fail "JetStream does not store on /data"
}

test_bundled_exposes_cnpg_secret_and_nats_service() {
  render --show-only templates/api/configmap.yaml
  expect_ok
  expect_line '  RANGELIGHT_API_POSTGRES_URL: "secret://rangelight-pg-app#uri"'
  expect_line '  RANGELIGHT_API_NATS_URL: "nats://rangelight-nats.rangelight-system.svc:4222"'
}

# --- AC3: your own Postgres and NATS --------------------------------------------

test_external_installs_only_the_api_settings() {
  render --api-versions "${CNPG_API}" "${EXTERNAL[@]}"
  expect_ok
  [[ "$(kinds)" == "ConfigMap/rangelight-api" ]] || fail "external mode renders more than the api ConfigMap: $(kinds | paste -sd' ')"
  expect_line '  RANGELIGHT_API_POSTGRES_URL: "secret://pg-api#uri"'
  expect_line '  RANGELIGHT_API_NATS_URL: "tls://nats.example.com:4222"'
}

test_external_postgres_alone_keeps_bundled_nats() {
  render --api-versions "${CNPG_API}" --set postgres.enabled=false --set postgres.external.secretRef.name=pg-api \
    --set postgres.external.secretRef.key=url
  expect_ok
  [[ "$(kinds | paste -sd' ')" == "ConfigMap/rangelight-api ConfigMap/rangelight-nats-config NetworkPolicy/rangelight-nats Service/rangelight-nats Service/rangelight-nats-headless StatefulSet/rangelight-nats" ]] ||
    fail "external Postgres with bundled NATS renders: $(kinds | paste -sd' ')"
  expect_line '  RANGELIGHT_API_POSTGRES_URL: "secret://pg-api#url"'
}

# --- AC5: NetworkPolicies --------------------------------------------------------

test_postgres_ingress_only_from_rangelight_and_the_operator() {
  render --show-only templates/postgres/networkpolicy.yaml
  expect_ok
  expect_line '      cnpg.io/cluster: "rangelight-pg"'
  [[ "$(grep -cxF -e '              app.kubernetes.io/part-of: "rangelight"' <<<"${OUT}")" -eq 1 ]] || fail "no part-of rule"
  expect_line '              app.kubernetes.io/name: "cloudnative-pg"'
  expect_no_match 'namespaceSelector|ipBlock'
}

test_nats_ingress_only_from_rangelight() {
  render --show-only templates/nats/networkpolicy.yaml
  expect_ok
  expect_line '      app.kubernetes.io/name: "nats"'
  expect_line '              app.kubernetes.io/part-of: "rangelight"'
  expect_line '          port: 4222'
  expect_no_match 'namespaceSelector|ipBlock|port: 8222'
}

test_our_objects_carry_the_labels() {
  local label
  render --api-versions "${CNPG_API}" --show-only templates/postgres/cluster.yaml --show-only templates/api/configmap.yaml \
    --show-only templates/postgres/networkpolicy.yaml --show-only templates/nats/networkpolicy.yaml
  expect_ok
  for label in name component part-of managed-by; do
    [[ "$(grep -cE -e "^    app.kubernetes.io/${label}: \"" <<<"${OUT}")" -eq 4 ]] || fail "an object without app.kubernetes.io/${label}"
  done
}

# --- values the chart refuses ------------------------------------------------------

test_refuses_external_postgres_without_secret() {
  render --set postgres.enabled=false
  expect_refused "/postgres/external/secretRef/name"
}

test_refuses_external_nats_without_url() {
  render --set nats.enabled=false
  expect_refused "/nats/external/url"
}

test_refuses_a_nats_url_that_is_not_nats() {
  render --set nats.enabled=false --set nats.external.url=http://nats.example.com:4222
  expect_refused "/nats/external/url"
}

test_refuses_an_image_without_a_digest() {
  render --set postgres.image=ghcr.io/cloudnative-pg/postgresql:18.6-minimal-trixie
  expect_refused "/postgres/image"
}

test_refuses_a_latest_tag() {
  render --set "postgres.image=ghcr.io/cloudnative-pg/postgresql:latest@sha256:$(printf '0%.0s' {1..64})"
  expect_refused "/postgres/image"
}

test_refuses_an_unknown_key() {
  render --set postgres.password=hunter2
  expect_refused "additional properties 'password' not allowed"
}

test_refuses_another_namespace() {
  STATUS=0
  OUT="$(helm template rangelight "${CHART}" --namespace default 2>&1)" || STATUS=$?
  expect_refused 'install into the namespace rangelight-system, not "default"'
}

main "$@"
