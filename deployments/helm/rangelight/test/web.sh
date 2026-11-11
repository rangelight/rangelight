#!/usr/bin/env bash
# What the chart renders for the Workbench (RAN-44), and the values it refuses. Run by `scripts/check.sh deploy`.
# Usage: deployments/helm/rangelight/test/web.sh [test_name...]   (no argument runs every test)
set -euo pipefail
# shellcheck source=deployments/helm/rangelight/test/lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

SERVE=(-f "${CHART}/test/serve-values.yaml" -f "${CHART}/test/ingress-values.yaml")

test_web_is_off_by_default() {
  render --api-versions "${CNPG_API}"
  expect_ok
  expect_no_match 'rangelight-web'
}

test_web_pod_is_restricted_pinned_with_one_replica() {
  render "${SERVE[@]}" --show-only templates/web/deployment.yaml --show-only templates/web/service.yaml
  expect_ok
  [[ "$(kinds | paste -sd' ')" == "Deployment/rangelight-web Service/rangelight-web" ]] || fail "the web renders: $(kinds | paste -sd' ')"
  expect_restricted
  expect_line '        runAsUser: 101'
  expect_line '  replicas: 1'
  expect_line "          image: \"ghcr.io/rangelight/web:test@sha256:$(printf '6%.0s' {1..64})\""
  expect_line '              path: "/"'
  expect_line '              containerPort: 8080'
  expect_line '              mountPath: "/tmp"'
  # No config, no secret: nothing to leak.
  expect_no_match '^ +(env|envFrom|secret|secretKeyRef|projected|configMapRef):'
}

test_web_network_policy_admits_only_the_ingress_and_allows_no_egress() {
  render "${SERVE[@]}" --show-only templates/web/networkpolicy.yaml
  expect_ok
  expect_line '    - "Egress"'
  expect_no_match '^  egress:'
  [[ "$(network_peers)" == "ingress 8080 ns:kubernetes.io/metadata.name=ingress-nginx pod:app.kubernetes.io/name=ingress-nginx" ]] ||
    fail "the web's peers:
$(network_peers)"
  # No ingress.from: nothing reaches it.
  render -f "${CHART}/test/serve-values.yaml" --show-only templates/web/networkpolicy.yaml
  expect_ok
  [[ -z "$(network_peers)" ]] || fail "the web admits: $(network_peers)"
}

test_refuses_a_web_without_a_pinned_image() {
  render --set web.enabled=true
  expect_refused "/web/image"
  render "${SERVE[@]}" --set web.image=ghcr.io/rangelight/web:1
  expect_refused "/web/image"
}

main "$@"
