#!/usr/bin/env bash
# What the chart renders for the one Ingress at publicUrl (RAN-44), and the values it refuses. Run by
# `scripts/check.sh deploy`.
# Usage: deployments/helm/rangelight/test/ingress.sh [test_name...]   (no argument runs every test)
set -euo pipefail
# shellcheck source=deployments/helm/rangelight/test/lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

SERVE=(-f "${CHART}/test/serve-values.yaml" -f "${CHART}/test/ingress-values.yaml")

# routes: the Ingress's paths as "<path> <service>:<port>", in order.
routes() {
  awk '/^          - path: / { p = $3 } /^                name: / { s = $2 } /^                  (name|number): / { print p, s ":" $2 }' <<<"${OUT}" | tr -d '"'
}

test_ingress_is_off_by_default() {
  render --api-versions "${CNPG_API}"
  expect_ok
  expect_no_match '^kind: "?Ingress"?$'
}

test_ingress_routes_the_api_dex_and_the_workbench() {
  render "${SERVE[@]}" --show-only templates/ingress.yaml --set publicUrl=https://rangelight.example.com:8443
  expect_ok
  expect_line '    - host: "rangelight.example.com"'
  [[ "$(routes)" == "/rangelight.auth.v1alpha1.AuthService rangelight-api:http
/rangelight.session.v1alpha1.SessionService rangelight-api:http
/auth rangelight-api:http
/v1alpha1/sessions rangelight-api:http
/dex rangelight-dex:5556
/ rangelight-web:http" ]] || fail "the Ingress routes:
$(routes)"
  [[ "$(grep -c 'pathType: "Prefix"' <<<"${OUT}")" -eq "$(routes | wc -l)" ]] || fail "a path that isn't a Prefix"
  expect_no_match 'ingressClassName|tls:|annotations:'
}

# Every Connect service in api/proto has its route: a new service can't go unrouted.
test_ingress_routes_every_connect_service() {
  local package service
  render "${SERVE[@]}" --show-only templates/ingress.yaml
  while read -r package service; do
    expect_line "          - path: \"/${package}.${service}\""
  done < <(find "${CHART}/../../../api/proto" -name '*.proto' -exec awk '/^package / { p = $2; sub(/;/, "", p) } /^service / { print p, $2 }' {} +)
}

test_ingress_without_dex_routes_no_dex() {
  render "${SERVE[@]}" "${EXTERNAL_OIDC[@]}" --set dex.github.org= --show-only templates/ingress.yaml
  expect_ok
  expect_no_match 'rangelight-dex|path: "/dex"'
}

test_ingress_class_tls_and_annotations_are_optional_and_as_given() {
  render "${SERVE[@]}" --show-only templates/ingress.yaml --set ingress.className=nginx --set ingress.tls.secretName=rangelight-tls \
    --set-string 'ingress.annotations.nginx\.ingress\.kubernetes\.io/proxy-read-timeout=3600'
  expect_ok
  expect_line '  ingressClassName: "nginx"'
  expect_line '        - "rangelight.example.com"'
  expect_line '      secretName: "rangelight-tls"'
  expect_line '    nginx.ingress.kubernetes.io/proxy-read-timeout: "3600"'
}

test_refuses_an_ingress_without_from_the_api_or_the_workbench() {
  render -f "${CHART}/test/serve-values.yaml" --set ingress.enabled=true
  expect_refused "/ingress/from/namespaceLabels"
  render "${SERVE[@]}" --set ingress.from.httpsPort=0
  expect_refused "/ingress/from/httpsPort"
  render -f "${CHART}/test/ingress-values.yaml"
  expect_refused "ingress.enabled needs api.enabled and web.enabled"
  render "${SERVE[@]}" --set web.enabled=false
  expect_refused "ingress.enabled needs api.enabled and web.enabled"
  render "${SERVE[@]}" --set ingress.className=Not_A_Class
  expect_refused "/ingress/className"
  render "${SERVE[@]}" --set ingress.tls.secretName=-bad
  expect_refused "/ingress/tls/secretName"
}

test_ingress_objects_carry_the_labels() {
  local label
  render "${SERVE[@]}" --show-only templates/ingress.yaml --show-only templates/api/deployment.yaml \
    --show-only templates/api/service.yaml --show-only templates/api/networkpolicy.yaml \
    --show-only templates/relay/deployment.yaml --show-only templates/relay/networkpolicy.yaml \
    --show-only templates/web/deployment.yaml --show-only templates/web/service.yaml --show-only templates/web/networkpolicy.yaml
  expect_ok
  for label in name component part-of managed-by; do
    [[ "$(grep -A6 -e '^  labels:$' <<<"${OUT}" | grep -cE -e "^    app.kubernetes.io/${label}: \"")" -eq 9 ]] || fail "an object without app.kubernetes.io/${label}"
  done
}

main "$@"
