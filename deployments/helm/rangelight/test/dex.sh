#!/usr/bin/env bash
# What the chart renders for sign-in (RAN-37): bundled Dex, or your own OIDC provider; and the values it
# refuses. Run by `scripts/check.sh deploy`.
# Usage: deployments/helm/rangelight/test/dex.sh [test_name...]   (no argument runs every test)
set -euo pipefail
# shellcheck source=deployments/helm/rangelight/test/lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

# --- RAN-37 AC1: sign-in through bundled Dex, or your own OIDC provider -----------

test_dex_bundled_pinned_restricted_with_secrets_from_secrets() {
  render --show-only charts/dex/templates/deployment.yaml
  expect_ok
  expect_line '          image: "ghcr.io/dexidp/dex:v2.46.1@sha256:65b72ca0f4b0bf6d4ada84ec21da8851782563404ac7b8f926923d27ceced9ab"'
  expect_line "  replicas: 1"
  expect_line "      serviceAccountName: rangelight-dex"
  expect_line "        app.kubernetes.io/part-of: rangelight"
  expect_restricted token
  expect_line "        runAsUser: 1001"
  [[ "$(grep -cE -e '^ +secretKeyRef:$' <<<"${OUT}")" -eq 3 ]] || fail "not every credential comes from a Secret"
  expect_line "                  name: rangelight-dex-github"
  expect_line "                  name: rangelight-oidc"
  expect_line "            secretName: rangelight-dex-config"
  expect_no_match '^ +value: '
  # RAN-53: Dex renders its config into /tmp, a memory volume; the root stays read-only.
  expect_line "            readOnlyRootFilesystem: true"
  [[ "$(grep -A1 -e '- mountPath: /tmp$' <<<"${OUT}" | tail -1)" == "              name: tmp" ]] ||
    fail "Dex without /tmp mounted from the tmp volume"
  expect_line "            medium: Memory"
  expect_line "            sizeLimit: 16Mi"
  render --show-only templates/dex/serviceaccount.yaml
  expect_ok
  expect_line "automountServiceAccountToken: false"
}

test_dex_lets_in_only_the_org_and_registers_the_api() {
  render --show-only templates/dex/config.yaml
  expect_ok
  expect_line '    issuer: "https://rangelight.example.com/dex"'
  expect_line '      type: "memory"'
  expect_line '    enablePasswordDB: false'
  expect_line '      - type: "github"'
  # shellcheck disable=SC2016 # Dex's own variables, kept literal in the config
  expect_line '          clientID: "$GITHUB_CLIENT_ID"'
  # shellcheck disable=SC2016 # Dex's own variables, kept literal in the config
  expect_line '          clientSecret: "$GITHUB_CLIENT_SECRET"'
  expect_line '          redirectURI: "https://rangelight.example.com/dex/callback"'
  expect_line '            - name: "rangelight"'
  expect_line '      - id: "rangelight-api"'
  expect_line '        secretEnv: "RANGELIGHT_OIDC_CLIENT_SECRET"'
  expect_line '        redirectURIs: ["https://rangelight.example.com/auth/callback"]'
  [[ "$(grep -c '^      - type: ' <<<"${OUT}")" -eq 1 ]] || fail "a connector besides GitHub"
}

# Security review L2: without ingress.from nothing reaches Dex (the api reaches it through the controller too);
# egress is kube-dns and HTTPS outside the cluster.
test_dex_network_policy() {
  render --show-only templates/dex/networkpolicy.yaml
  expect_ok
  expect_line '      app.kubernetes.io/name: "dex"'
  expect_line '    - "Ingress"'
  [[ "$(network_peers)" == "egress 53 ns:kubernetes.io/metadata.name=kube-system pod:k8s-app=kube-dns
egress 443 ip:0.0.0.0/0-10.42.0.0/16-10.43.0.0/16" ]] || fail "Dex's peers:
$(network_peers)"
}

# Security review round 2, item 3: DNS egress follows the one chart-wide clusterDns setting.
test_dex_dns_egress_follows_cluster_dns() {
  render --show-only templates/dex/networkpolicy.yaml --set clusterDns.namespace=dns --set clusterDns.podLabel=k8s-app=coredns
  expect_ok
  expect_line '              kubernetes.io/metadata.name: "dns"'
  expect_line '              k8s-app: "coredns"'
  expect_no_match 'kube-dns|kube-system'
}

test_dex_network_policy_lets_in_the_configured_ingress() {
  render --show-only templates/dex/networkpolicy.yaml \
    --set 'ingress.from.namespaceLabels.kubernetes\.io/metadata\.name=ingress-nginx' \
    --set 'ingress.from.podLabels.app\.kubernetes\.io/name=ingress-nginx' \
    --set 'clusterCidrs={10.244.0.0/16,10.96.0.0/12}'
  expect_ok
  expect_line '              app.kubernetes.io/name: "ingress-nginx"'
  expect_line '              kubernetes.io/metadata.name: "ingress-nginx"'
  expect_line '              - "10.244.0.0/16"'
  expect_line '              - "10.96.0.0/12"'
  expect_no_match '10\.42\.0\.0'
  [[ "$(network_peers)" == "ingress 5556 ns:kubernetes.io/metadata.name=ingress-nginx pod:app.kubernetes.io/name=ingress-nginx
egress 53 ns:kubernetes.io/metadata.name=kube-system pod:k8s-app=kube-dns
egress 443 ip:0.0.0.0/0-10.244.0.0/16-10.96.0.0/12" ]] || fail "Dex's peers:
$(network_peers)"
  # Namespace labels alone: every pod in those namespaces.
  render --show-only templates/dex/networkpolicy.yaml \
    --set 'ingress.from.namespaceLabels.kubernetes\.io/metadata\.name=ingress-nginx'
  expect_ok
  expect_line '        - podSelector: {}'
}

test_refuses_dropping_or_widening_the_dex_network_policy() {
  render --set dex.networkPolicy.enabled=true
  expect_refused "/dex/networkPolicy/enabled"
  render --set 'clusterCidrs=null'
  expect_refused "missing property 'clusterCidrs'"
  render --set 'clusterCidrs[0]=everything'
  expect_refused "/clusterCidrs/0"
  render --set 'ingress.from.podLabels.a=bad value'
  expect_refused "/ingress/from/podLabels/a"
  # Helm's validator reports a bad label key without its path.
  render --set 'ingress.from.namespaceLabels.bad\ key=x'
  expect_refused "invalid propertyName 'bad key'"
}

test_api_signs_in_at_bundled_dex() {
  render --show-only templates/api/configmap.yaml
  expect_ok
  expect_line '  RANGELIGHT_API_PUBLIC_URL: "https://rangelight.example.com"'
  expect_line '  RANGELIGHT_API_OIDC_ISSUER: "https://rangelight.example.com/dex"'
  expect_line '  RANGELIGHT_API_OIDC_CLIENT_ID: "rangelight-api"'
  expect_line '  RANGELIGHT_API_OIDC_CLIENT_SECRET: "secret://rangelight-oidc#client-secret"'
  expect_line '  RANGELIGHT_API_OIDC_MEMBER_CLAIM: "preferred_username"'
}

test_external_oidc_installs_no_dex() {
  render "${EXTERNAL_OIDC[@]}" --set dex.github.org=
  expect_ok
  expect_no_match 'dex'
  render "${EXTERNAL_OIDC[@]}" --set oidc.clientId=rangelight --show-only templates/api/configmap.yaml
  expect_ok
  expect_line '  RANGELIGHT_API_OIDC_ISSUER: "https://idp.example.com/realms/eng"'
  expect_line '  RANGELIGHT_API_OIDC_CLIENT_ID: "rangelight"'
  expect_line '  RANGELIGHT_API_OIDC_CLIENT_SECRET: "secret://idp-client#client-secret"'
  render "${EXTERNAL_OIDC[@]}" --set oidc.memberClaim=rangelight_member --show-only templates/api/configmap.yaml
  expect_ok
  expect_line '  RANGELIGHT_API_OIDC_MEMBER_CLAIM: "rangelight_member"'
}

# Review M2: Dex with GitHub sets preferred_username to the login; any other Dex claim is the
# user-editable display name or missing.
test_refuses_another_member_claim_with_bundled_dex() {
  render --set oidc.memberClaim=name
  expect_refused "/oidc/memberClaim"
  render "${EXTERNAL_OIDC[@]}" --set 'oidc.memberClaim=a b'
  expect_refused "/oidc/memberClaim"
}

# --- sign-in values the chart refuses ----------------------------------------------

test_refuses_a_missing_or_unsafe_public_url() {
  local url
  for url in "" http://rangelight.example.com https://rangelight.example.com/ https://rangelight.example.com/app \
    "https://rangelight.example.com\"x"; do
    render --set-string "publicUrl=${url}"
    expect_refused "/publicUrl"
  done
}

test_refuses_bundled_dex_without_an_org() {
  render --set dex.github.org=
  expect_refused "/dex/github/org"
  render --set dex.github.org=-bad-
  expect_refused "/dex/github/org"
}

test_refuses_bundled_dex_with_another_client_secret() {
  render --set oidc.clientSecretRef.name=elsewhere
  expect_refused "/oidc/clientSecretRef/name"
}

test_refuses_your_own_provider_without_an_https_issuer() {
  render --set dex.enabled=false
  expect_refused "/oidc/issuer"
  render --set dex.enabled=false --set oidc.issuer=http://idp.example.com
  expect_refused "/oidc/issuer"
}

test_refuses_a_credential_as_a_value() {
  render --set 'dex.envVars[0].name=GITHUB_CLIENT_SECRET' --set 'dex.envVars[0].value=hunter2'
  expect_refused "/dex/envVars/0"
  render --set dex.config.issuer=https://elsewhere.example.com
  expect_refused "/dex/config"
}

test_refuses_more_dex_servers_or_a_latest_tag() {
  render --set dex.replicaCount=2
  expect_refused "/dex/replicaCount"
  render --set dex.image.repository=ghcr.io/dexidp/dex:latest
  expect_refused "/dex/image/repository"
}

test_dex_objects_carry_the_labels() {
  local label
  render --show-only templates/dex/config.yaml --show-only templates/dex/serviceaccount.yaml \
    --show-only templates/dex/networkpolicy.yaml
  expect_ok
  for label in name component part-of managed-by; do
    [[ "$(grep -cE -e "^    app.kubernetes.io/${label}: \"" <<<"${OUT}")" -eq 3 ]] || fail "an object without app.kubernetes.io/${label}"
  done
}

main "$@"
