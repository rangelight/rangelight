#!/usr/bin/env bash
# What the chart renders for NATS sign-in (RAN-43, ADR 0021 amendment): the keys Job, the bundled server's auth.conf,
# each workload's token, and the values it refuses. Run by `scripts/check.sh deploy`.
# Usage: deployments/helm/rangelight/test/nats.sh [test_name...]   (no argument runs every test)
set -euo pipefail
# shellcheck source=deployments/helm/rangelight/test/lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

SERVE=(-f "${CHART}/test/serve-values.yaml")
RUNNER=(-f "${CHART}/test/runner-values.yaml")
NATS=(--show-only charts/nats/templates/stateful-set.yaml --show-only charts/nats/templates/config-map.yaml)

# The keys Job and what it needs are hooks, created before anything else on install and upgrade, and deleted once done.
test_keys_job_runs_first_on_install_and_upgrade() {
  render --show-only templates/nats/keys.yaml
  expect_ok
  [[ "$(kinds | paste -sd' ')" == "ConfigMap/rangelight-nats-keys Job/rangelight-nats-keys Role/rangelight-nats-keys RoleBinding/rangelight-nats-keys ServiceAccount/rangelight-nats-keys" ]] ||
    fail "the keys Job renders: $(kinds | paste -sd' ')"
  [[ "$(grep -c '^    helm.sh/hook: "pre-install,pre-upgrade"$' <<<"${OUT}")" -eq 5 ]] || fail "not every object is a hook"
  [[ "$(grep -c '^    helm.sh/hook-weight: "-10"$' <<<"${OUT}")" -eq 4 ]] || fail "the Job's dependencies aren't first"
  expect_line '    helm.sh/hook-weight: "0"'
  expect_line '    helm.sh/hook-delete-policy: "before-hook-creation,hook-succeeded"'
  # It needs its own token for the API server.
  expect_restricted token
  expect_line "          image: \"$(awk '/keysImage:/ { gsub(/"/, "", $2); print $2 }' "${CHART}/values.yaml")\""
  # The script and the labels of the Secrets it creates.
  expect_line '  keys.sh: |'
  grep -qF -e '    create rangelight-nats-auth issuer=issuer.pub auth-user=auth-user.pub' <<<"${OUT}" ||
    fail "the ConfigMap doesn't hold files/nats/keys.sh"
  expect_line '      "app.kubernetes.io/part-of": "rangelight",'
}

# Create Secrets, read only the two it makes; nothing else.
test_keys_job_reads_only_its_two_secrets() {
  render --show-only templates/nats/keys.yaml
  expect_ok
  [[ "$(awk '/^rules:/ { on = 1; next } /^---/ { on = 0 } on' <<<"${OUT}" | paste -sd'|')" == \
    '  - apiGroups: [""]|    resources: ["secrets"]|    verbs: ["create"]|  - apiGroups: [""]|    resources: ["secrets"]|    resourceNames: ["rangelight-api-nats", "rangelight-nats-auth"]|    verbs: ["get"]' ]] ||
    fail "the Role: $(awk '/^rules:/ { on = 1; next } /^---/ { on = 0 } on' <<<"${OUT}")"
}

# The bundled server includes auth.conf from its ConfigMap, exactly files/nats/auth.conf, and reads its public keys from
# the keys Job's Secret.
test_the_bundled_server_includes_the_sign_in_config() {
  render "${NATS[@]}" --show-only templates/nats/auth.yaml
  expect_ok
  grep -qE -e '^ +include \.\./nats-auth/auth\.conf;?$' <<<"${OUT}" || fail "nats.conf doesn't include auth.conf"
  expect_line '              name: rangelight-nats-auth'
  [[ "$(grep -cE -e '^ +key: (issuer|auth-user)$' <<<"${OUT}")" -eq 2 ]] || fail "the server doesn't read both public keys"
  grep -qE -e '^ +(- )?mountPath: /etc/nats-auth$' <<<"${OUT}" || fail "auth.conf isn't mounted at /etc/nats-auth"
  grep -qE -e '^ +name: rangelight-nats-auth-config$' <<<"${OUT}" || fail "the server doesn't mount the ConfigMap"
  [[ "$(awk '/^  auth.conf: \|$/ { on = 1; next } on { sub(/^    /, ""); print }' <<<"${OUT}")" == \
    "$(cat "${CHART}/files/nats/auth.conf")" ]] || fail "the ConfigMap isn't files/nats/auth.conf"
}

# GitOps or `helm template`: your Secret, no Job; the server and the api read it.
test_an_existing_secret_replaces_the_job() {
  render "${SERVE[@]}" "${NATS[@]}" --show-only templates/api/deployment.yaml --show-only templates/api/configmap.yaml \
    --set nats.auth.existingSecret=nats-keys
  expect_ok
  ! kinds | grep -q 'rangelight-nats-keys' || fail "the Job still renders"
  [[ "$(grep -cE -e '^ +name: nats-keys$' <<<"${OUT}")" -eq 2 ]] || fail "the server doesn't read nats-keys"
  expect_line '                      path: "nats-keys/issuer-seed"'
  expect_line '                      path: "nats-keys/auth-user-seed"'
  expect_line '    issuer_seed = "secret://nats-keys#issuer-seed"'
  expect_line '    auth_user = "secret://nats-keys#auth-user-seed"'
}

# Each workload signs in with its own projected token, audience rangelight-nats, one hour.
test_each_workload_mounts_its_own_nats_token() {
  local template
  for template in api relay runner; do
    render "${SERVE[@]}" "${RUNNER[@]}" --show-only "templates/${template}/deployment.yaml"
    expect_ok
    [[ "$(grep -A2 -E -e '^ +audience: "rangelight-nats"$' <<<"${OUT}" | grep -cxE -e ' +(audience: "rangelight-nats"|expirationSeconds: 3600|path: "token")')" -eq 3 ]] ||
      fail "the ${template} has no NATS token"
    expect_line '              mountPath: "/var/run/secrets/rangelight/nats"'
  done
  render "${SERVE[@]}" --show-only templates/api/deployment.yaml --show-only templates/relay/deployment.yaml
  expect_line '      serviceAccountName: "rangelight-api"'
  expect_line '      serviceAccountName: "rangelight-relay"'
}

# The api verifies tokens against this cluster's issuer, its keys fetched with its own token over the cluster CA; its
# ServiceAccount may read the issuer and nothing else.
test_the_api_reads_the_cluster_issuer() {
  render "${SERVE[@]}" --show-only templates/api/configmap.yaml --show-only templates/api/serviceaccount.yaml \
    --set nats.auth.issuer=https://oidc.example.com/id/1 --set runner.namespace=lanes
  expect_ok
  expect_line '    url = "https://oidc.example.com/id/1"'
  expect_line '    runner_service_account = "lanes:runner"'
  expect_line '    account = "RL"'
  expect_line '  name: "system:service-account-issuer-discovery"'
  expect_line 'automountServiceAccountToken: false'
  render "${SERVE[@]}" --show-only templates/api/deployment.yaml
  expect_line '                  name: "kube-root-ca.crt"'
  expect_line '              mountPath: "/var/run/secrets/rangelight/kubernetes"'
  expect_line '              mountPath: "/etc/rangelight"'
}

# Your own server in operator mode: the accounts are public keys, and the api also mounts LANE's signing key.
test_operator_mode_names_both_accounts_and_mounts_the_lane_key() {
  local rl lane
  rl="A$(printf 'B%.0s' {1..55})" lane="A$(printf 'C%.0s' {1..55})"
  render "${SERVE[@]}" --show-only templates/api/deployment.yaml --show-only templates/api/configmap.yaml \
    --set nats.enabled=false --set nats.external.url=tls://nats.example.com:4222 --set nats.auth.existingSecret=nats-keys \
    --set "nats.auth.account=${rl}" --set "nats.auth.laneAccount=${lane}"
  expect_ok
  expect_line "    account = \"${rl}\""
  expect_line "    lane_account = \"${lane}\""
  expect_line '    lane_issuer_seed = "secret://nats-keys#lane-issuer-seed"'
  expect_line '                      path: "nats-keys/lane-issuer-seed"'
  render "${SERVE[@]}" --show-only templates/api/configmap.yaml
  expect_line '    lane_account = "LANE"'
  expect_no_match 'lane_issuer_seed'
}

test_refuses_a_token_in_clear_text_or_your_own_nats_without_keys() {
  render --set nats.enabled=false --set nats.external.url=tls://nats.example.com:4222
  expect_refused "/nats/auth/existingSecret"
  render --set nats.enabled=false --set nats.external.url=nats://nats.example.com:4222 --set nats.auth.existingSecret=k
  expect_refused "/nats/external/url"
  render --set nats.enabled=false --set nats.external.url=nats://nats.messaging.svc:4222 --set nats.auth.existingSecret=k
  expect_ok
  render --set nats.auth.account=ACCOUNT
  expect_refused "/nats/auth/account"
  render --set nats.auth.laneAccount=RL
  expect_refused "/nats/auth/laneAccount"
  render --set nats.auth.issuer=http://kubernetes.default.svc
  expect_refused "/nats/auth/issuer"
}

# AC3: no credential lives in values or in what the chart renders: the keys are made in the cluster, or are yours.
test_ac3_no_nats_credential_in_values_or_the_render() {
  local on=(-f "${CHART}/test/serve-values.yaml" -f "${CHART}/test/runner-values.yaml" -f "${CHART}/test/git-mirror-values.yaml")
  render --api-versions "${CNPG_API}" "${on[@]}"
  expect_ok
  ! kinds | grep -q '^Secret/.*nats' || fail "the chart renders a NATS Secret: $(kinds | grep '^Secret/' | paste -sd' ')"
  # An nkey seed is S, a key type letter and 56 base32 characters.
  expect_no_match 'S[ACNOPUX][A-Z2-7]{56}'
  ! grep -qE -e 'S[ACNOPUX][A-Z2-7]{56}' "${CHART}/values.yaml" || fail "a seed in values.yaml"
}

main "$@"
