#!/usr/bin/env bash
# What the chart renders for the api (RAN-44), and the values it refuses. Run by `scripts/check.sh deploy`.
# Usage: deployments/helm/rangelight/test/api.sh [test_name...]   (no argument runs every test)
set -euo pipefail
# shellcheck source=deployments/helm/rangelight/test/lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

SERVE=(-f "${CHART}/test/serve-values.yaml" -f "${CHART}/test/ingress-values.yaml")

test_api_is_off_by_default() {
  render --api-versions "${CNPG_API}"
  expect_ok
  [[ "$(kinds | grep -c 'rangelight-api')" -eq 1 ]] || fail "the api renders more than its ConfigMap: $(kinds | paste -sd' ')"
}

test_api_pod_is_restricted_pinned_with_one_replica() {
  render "${SERVE[@]}" --show-only templates/api/deployment.yaml --show-only templates/api/service.yaml
  expect_ok
  [[ "$(kinds | paste -sd' ')" == "Deployment/rangelight-api Service/rangelight-api" ]] || fail "the api renders: $(kinds | paste -sd' ')"
  expect_restricted
  expect_line '  replicas: 1'
  expect_line "          image: \"ghcr.io/rangelight/api:test@sha256:$(printf '4%.0s' {1..64})\""
  expect_line '                name: "rangelight-api"'
  expect_line '              path: "/healthz"'
  [[ "$(grep -A6 -e '^          startupProbe:$' <<<"${OUT}" | grep -cxE -e ' +(path: "/healthz"|periodSeconds: 2|failureThreshold: 60)')" -eq 3 ]] ||
    fail "no startupProbe on /healthz every 2s, 60 times"
  expect_line '              containerPort: 8080'
  expect_line '      targetPort: "http"'
}

# Secrets by reference: each key as a file where secret://<name>#<key> resolves, never an environment value.
test_api_mounts_exactly_its_three_secret_keys() {
  local files
  render "${SERVE[@]}" --show-only templates/api/deployment.yaml
  expect_ok
  files="$(grep -E '^ +path: "[^/]+/[^/]+"$' <<<"${OUT}" | tr -d ' "' | sed 's/^path://' | paste -sd' ')"
  [[ "${files}" == "rangelight-pg-app/uri rangelight-oidc/client-secret rangelight-linear/api-key" ]] || fail "the api mounts: ${files}"
  expect_line '              mountPath: "/var/run/rangelight/secrets"'
  # Not optional: the pod doesn't start until every Secret and key exists.
  expect_no_match 'optional:'
  expect_no_match 'secretKeyRef|^ +value: '
  render "${SERVE[@]}" --show-only templates/api/deployment.yaml --set postgres.enabled=false \
    --set postgres.external.secretRef.name=pg-api --set linear.apiKeySecretRef.key=key
  expect_line '                      path: "pg-api/uri"'
  expect_line '                      path: "rangelight-linear/key"'
}

# RAN-54: your own CA bundle, mounted like the other Secret refs and named in the config; nothing by default.
test_api_trusts_an_extra_ca_bundle_only_when_set() {
  local files
  render "${SERVE[@]}" --show-only templates/api/configmap.yaml
  expect_ok
  expect_no_match 'EXTRA_CA'
  render "${SERVE[@]}" --show-only templates/api/configmap.yaml --set tls.extraCaSecretRef.name=corp-ca
  expect_line '  RANGELIGHT_API_EXTRA_CA_FILE: "/var/run/rangelight/secrets/corp-ca/ca.crt"'
  render "${SERVE[@]}" --show-only templates/api/deployment.yaml --set tls.extraCaSecretRef.name=corp-ca \
    --set tls.extraCaSecretRef.key=bundle.pem
  expect_ok
  files="$(grep -E '^ +path: "[^/]+/[^/]+"$' <<<"${OUT}" | tr -d ' "' | sed 's/^path://' | paste -sd' ')"
  [[ "${files}" == "rangelight-pg-app/uri rangelight-oidc/client-secret rangelight-linear/api-key corp-ca/bundle.pem" ]] ||
    fail "the api mounts: ${files}"
  render "${SERVE[@]}" --set tls.extraCaSecretRef.name=Corp_CA
  expect_refused "/tls/extraCaSecretRef/name"
}

test_api_config_names_linear_and_rolls_the_pod() {
  local before
  render "${SERVE[@]}" --show-only templates/api/configmap.yaml
  expect_ok
  expect_line '  RANGELIGHT_API_LINEAR_TEAM: "RAN"'
  expect_line '  RANGELIGHT_API_LINEAR_API_KEY: "secret://rangelight-linear#api-key"'
  render "${SERVE[@]}" --show-only templates/api/deployment.yaml
  before="$(grep 'checksum/config' <<<"${OUT}")"
  render "${SERVE[@]}" --show-only templates/api/deployment.yaml --set linear.team=OPS
  [[ -n "${before}" && "$(grep 'checksum/config' <<<"${OUT}")" != "${before}" ]] || fail "a config change doesn't roll the api"
}

test_api_network_policy() {
  render "${SERVE[@]}" --show-only templates/api/networkpolicy.yaml --set ingress.from.httpsPort=8443
  expect_ok
  [[ "$(network_peers)" == "ingress 8080 ns:kubernetes.io/metadata.name=ingress-nginx pod:app.kubernetes.io/name=ingress-nginx
egress 53 ns:kubernetes.io/metadata.name=kube-system pod:k8s-app=kube-dns
egress 5432 pod:cnpg.io/cluster=rangelight-pg
egress 4222 pod:app.kubernetes.io/name=nats,app.kubernetes.io/instance=rangelight
egress 8443 ns:kubernetes.io/metadata.name=ingress-nginx pod:app.kubernetes.io/name=ingress-nginx
egress 443 ip:0.0.0.0/0-10.42.0.0/16-10.43.0.0/16" ]] || fail "the api's peers:
$(network_peers)"
}

# Your own issuer on another port: the HTTPS rule allows that port too, and the api needs no route to the controller.
test_api_reaches_its_own_issuer_on_its_port() {
  render "${SERVE[@]}" --show-only templates/api/networkpolicy.yaml "${EXTERNAL_OIDC[@]}" \
    --set oidc.issuer=https://idp.example.com:8443/realms/eng
  expect_ok
  [[ "$(network_peers | tail -1)" == "egress 443,8443 ip:0.0.0.0/0-10.42.0.0/16-10.43.0.0/16" ]] ||
    fail "the api's HTTPS egress: $(network_peers | tail -1)"
  ! network_peers | grep -q '^egress .*ingress-nginx' || fail "the api reaches the controller without bundled Dex"
  render "${SERVE[@]}" --show-only templates/api/networkpolicy.yaml "${EXTERNAL_OIDC[@]}" \
    --set oidc.issuer=https://idp.example.com:443
  [[ "$(network_peers | tail -1)" == "egress 443 ip:0.0.0.0/0-10.42.0.0/16-10.43.0.0/16" ]] ||
    fail "the api's HTTPS egress: $(network_peers | tail -1)"
}

test_api_network_policy_with_external_services_and_no_ingress() {
  # No Ingress, no ingress.from: nothing may reach the api.
  render -f "${CHART}/test/serve-values.yaml" --show-only templates/api/networkpolicy.yaml \
    --set postgres.enabled=false --set postgres.external.secretRef.name=pg --set nats.enabled=false \
    --set nats.external.url=tls://nats.example.com:7422 "${EXTERNAL_OIDC[@]}"
  expect_ok
  [[ "$(network_peers)" == "egress 53 ns:kubernetes.io/metadata.name=kube-system pod:k8s-app=kube-dns
egress 5432 any
egress 7422 any
egress 443 ip:0.0.0.0/0-10.42.0.0/16-10.43.0.0/16" ]] || fail "the api's peers:
$(network_peers)"
  render "${SERVE[@]}" --show-only templates/api/networkpolicy.yaml --set nats.enabled=false --set nats.external.url=nats://nats
  expect_line '          port: 4222'
}

test_refuses_an_api_without_its_image_or_linear() {
  render -f "${CHART}/test/serve-values.yaml" --set api.image=
  expect_refused "/api/image"
  render "${SERVE[@]}" --set api.image=ghcr.io/rangelight/api:1
  expect_refused "/api/image"
  render "${SERVE[@]}" --set linear.team=
  expect_refused "/linear/team"
  render "${SERVE[@]}" --set linear.team=RAN-1
  expect_refused "/linear/team"
  render "${SERVE[@]}" --set linear.apiKeySecretRef.name=
  expect_refused "/linear/apiKeySecretRef/name"
}

main "$@"
