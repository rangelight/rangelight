#!/usr/bin/env bash
# What the chart renders for the relay (RAN-44), and the values it refuses. Run by `scripts/check.sh deploy`.
# Usage: deployments/helm/rangelight/test/relay.sh [test_name...]   (no argument runs every test)
set -euo pipefail
# shellcheck source=deployments/helm/rangelight/test/lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

SERVE=(-f "${CHART}/test/serve-values.yaml")

test_relay_is_off_by_default() {
  render --api-versions "${CNPG_API}"
  expect_ok
  expect_no_match 'rangelight-relay'
}

test_relay_pod_is_restricted_pinned_single_writer() {
  render "${SERVE[@]}" --show-only templates/relay/deployment.yaml
  expect_ok
  expect_restricted
  expect_line '  replicas: 1'
  expect_line '    type: "Recreate"'
  expect_line "          image: \"ghcr.io/rangelight/relay:test@sha256:$(printf '5%.0s' {1..64})\""
  expect_line '              path: "/healthz"'
  [[ "$(grep -A6 -e '^          startupProbe:$' <<<"${OUT}" | grep -cxE -e ' +(path: "/healthz"|periodSeconds: 2|failureThreshold: 60)')" -eq 3 ]] ||
    fail "no startupProbe on /healthz every 2s, 60 times"
}

# The relay's config keys (configs/relay.default.toml), with the database a mounted Secret file.
test_relay_settings_are_its_config_keys() {
  local key
  render "${SERVE[@]}" --show-only templates/relay/deployment.yaml --set api.streamReplicas=3
  expect_ok
  expect_line '              value: "secret://rangelight-pg-app#uri"'
  expect_line '              value: "nats://rangelight-nats.rangelight-system.svc:4222"'
  expect_line '              value: "3"'
  expect_line '                      path: "rangelight-pg-app/uri"'
  [[ "$(grep -c '^ *- secret:$' <<<"${OUT}")" -eq 1 ]] || fail "the relay mounts more than the database Secret"
  for key in $(grep -oE '"RANGELIGHT_RELAY_[A-Z_]+"' <<<"${OUT}" | tr -d '"' | sed 's/^RANGELIGHT_RELAY_//' | tr '[:upper:]' '[:lower:]'); do
    grep -qE "^    pub ${key}: " "${CHART}/../../../services/relay/src/config.rs" || fail "RANGELIGHT_RELAY_${key^^} isn't a relay config key"
  done
  render "${SERVE[@]}" --show-only templates/relay/deployment.yaml --set postgres.enabled=false \
    --set postgres.external.secretRef.name=pg-api --set nats.enabled=false --set nats.external.url=tls://nats.example.com:4222
  expect_line '              value: "secret://pg-api#uri"'
  expect_line '              value: "tls://nats.example.com:4222"'
}

test_relay_network_policy_admits_nothing_and_reaches_only_its_stores() {
  render "${SERVE[@]}" -f "${CHART}/test/ingress-values.yaml" --show-only templates/relay/networkpolicy.yaml
  expect_ok
  expect_line '    - "Ingress"'
  [[ "$(network_peers)" == "egress 53 ns:kubernetes.io/metadata.name=kube-system pod:k8s-app=kube-dns
egress 5432 pod:cnpg.io/cluster=rangelight-pg
egress 4222 pod:app.kubernetes.io/name=nats,app.kubernetes.io/instance=rangelight" ]] || fail "the relay's peers:
$(network_peers)"
}

test_refuses_a_relay_without_a_pinned_image() {
  render --set relay.enabled=true
  expect_refused "/relay/image"
  render "${SERVE[@]}" --set relay.image=ghcr.io/rangelight/relay:latest
  expect_refused "/relay/image"
}

main "$@"
