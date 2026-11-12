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

# auth_conf [chart]: the auth.conf text in templates/nats/_auth.conf.tpl, between its define and end lines.
auth_conf() {
  awk '/^\{\{- define "rangelight.natsAuthConf"/ { on = 1; next } /^\{\{ end \}\}$/ { on = 0 } on' \
    "${1:-${CHART}}/templates/nats/_auth.conf.tpl"
}

# The bundled server includes auth.conf from its ConfigMap, exactly templates/nats/_auth.conf.tpl's, and reads its public keys from
# the keys Job's Secret.
test_the_bundled_server_includes_the_sign_in_config() {
  render "${NATS[@]}" --show-only templates/nats/auth.yaml
  expect_ok
  grep -qE -e '^ +include \.\./nats-auth/auth\.conf;?$' <<<"${OUT}" || fail "nats.conf doesn't include auth.conf"
  expect_line '              name: rangelight-nats-auth'
  [[ "$(grep -cE -e '^ +key: (issuer|auth-user)$' <<<"${OUT}")" -eq 2 ]] || fail "the server doesn't read both public keys"
  grep -qE -e '^ +(- )?mountPath: /etc/nats-auth$' <<<"${OUT}" || fail "auth.conf isn't mounted at /etc/nats-auth"
  grep -qE -e '^ +name: rangelight-nats-auth-config$' <<<"${OUT}" || fail "the server doesn't mount the ConfigMap"
  [[ "$(awk '/^  auth.conf: \|$/ { on = 1; next } /^  [a-z]/ { on = 0 } on { sub(/^    /, ""); print }' <<<"${OUT}")" == \
    "$(auth_conf)" ]] || fail "the ConfigMap isn't templates/nats/_auth.conf.tpl's auth.conf"
}

# auth_checksum: the rendered NATS pod's checksum/auth annotation, from OUT.
auth_checksum() { awk '/^ +checksum\/auth: / { print $2 }' <<<"${OUT}"; }

# checksum_of <chart>: auth_checksum of the NATS pod that chart renders.
checksum_of() {
  local CHART="$1"
  render --show-only charts/nats/templates/stateful-set.yaml
  auth_checksum
}

# RAN-70: the NATS pod carries the sha256 of the auth.conf its ConfigMap holds, so a changed auth.conf rolls the server
# on upgrade; the same auth.conf, the same annotation.
test_a_changed_auth_conf_rolls_the_server() {
  local first copy changed
  render --show-only charts/nats/templates/stateful-set.yaml
  expect_ok
  first="$(auth_checksum)"
  [[ "${first}" == "$(auth_conf | sha256sum | cut -d' ' -f1)" ]] || fail "checksum/auth isn't auth.conf's sha256: ${first}"
  render --show-only charts/nats/templates/stateful-set.yaml
  [[ "$(auth_checksum)" == "${first}" ]] || fail "checksum/auth changes between two renders of the same chart"
  copy="$(mktemp -d /tmp/rl-chart-auth.XXXXXX)"
  cp -r "${CHART}/." "${copy}"
  sed -i 's/^max_payload: 131072$/max_payload: 65536/' "${copy}/templates/nats/_auth.conf.tpl"
  changed="$(checksum_of "${copy}")"
  rm -rf "${copy}"
  [[ -n "${changed}" && "${changed}" != "${first}" ]] || fail "a changed auth.conf keeps checksum/auth: ${changed}"
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
  # RAN-66: a bare .svc, which the lanes can't resolve (they look names up as given).
  render --set nats.enabled=false --set nats.external.url=nats://nats.messaging.svc:4222 --set nats.auth.existingSecret=k
  expect_refused "/nats/external/url"
  # RAN-66 security review M1: one server, no query, a real port.
  # The comma is escaped: helm --set would split it.
  for url in "nats://evil.com\,x.svc.cluster.local" "nats://x.ns.svc.cluster.local?x" "nats://x.ns.svc.cluster.local:0"; do
    render --set nats.enabled=false --set "nats.external.url=${url}" --set nats.auth.existingSecret=k
    expect_refused "/nats/external/url"
  done
  render --set nats.enabled=false --set nats.external.url=nats://nats.messaging.svc.cluster.local:4222 --set nats.auth.existingSecret=k
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

# RAN-78: the NATS pod runs files/nats/archive.sh before the server, from the image the server runs, restricted, with the
# JetStream volume and the auth ConfigMap that holds the script.
test_an_init_container_archives_orphans_before_the_server() {
  render "${NATS[@]}" --show-only templates/nats/auth.yaml
  expect_ok
  expect_line '  archive.sh: |'
  [[ "$(awk '/^  archive.sh: \|$/ { on = 1; next } /^  [a-z]/ { on = 0 } on { sub(/^    /, ""); print }' <<<"${OUT}")" == \
    "$(cat "${CHART}/files/nats/archive.sh")" ]] || fail "the ConfigMap doesn't hold files/nats/archive.sh"
  local init
  init="$(awk '/^      initContainers:$/ { on = 1; next } /^      [a-z]/ { on = 0 } on' <<<"${OUT}")"
  grep -qxF -e '      - command:' <<<"${init}" || fail "no init container"
  [[ "$(grep -cxE -e '        - (sh|/etc/nats-auth/archive.sh|/data/jetstream|/etc/nats-auth/auth.conf)' <<<"${init}")" -eq 4 ]] ||
    fail "the init container doesn't run archive.sh on /data/jetstream: ${init}"
  [[ "$(grep -c "image: $(awk '/fullImageName:/ { gsub(/"/, "", $2); print $2 }' "${CHART}/values.yaml")$" <<<"${OUT}")" -eq 2 ]] ||
    fail "the init container doesn't run the server's own image"
  grep -qxF -e '          readOnlyRootFilesystem: true' <<<"${init}" || fail "the init container isn't read-only"
  grep -qxF -e '            - ALL' <<<"${init}" || fail "the init container keeps capabilities"
  grep -qxF -e '          name: rangelight-nats-js' <<<"${init}" || fail "the init container doesn't mount the JetStream volume"
}

ARCHIVE="${CHART}/files/nats/archive.sh"

# g_store <dir> <stream...>: <dir>/jetstream with those streams in $G (as directories), RL's agent stream, and
# <dir>/auth.conf naming RL.
g_store() {
  local dir="$1" stream
  shift
  mkdir -p "${dir}/jetstream/RL/streams/rl-default-agent"
  for stream in "$@"; do mkdir -p "${dir}/jetstream/\$G/streams/${stream}"; done
  printf 'accounts {\n  RL {\n    jetstream: enabled\n  }\n}\n' >"${dir}/auth.conf"
}

# AC2: Rangelight's streams leave $G for an archive beside the store, once; RL stays; an existing archive is kept.
test_archive_moves_rangelight_s_g_streams_once() {
  local dir out
  dir="$(mktemp -d /tmp/rl-test-archive.XXXXXX)"
  g_store "${dir}" rl-default-desired rl-default-status
  out="$(sh "${ARCHIVE}" "${dir}/jetstream" "${dir}/auth.conf")"
  [[ ! -e "${dir}/jetstream/\$G" ]] || fail "\$G is still in the store"
  [[ -d "${dir}/jetstream/RL/streams/rl-default-agent" ]] || fail "RL was touched"
  [[ "$(find "${dir}" -maxdepth 3 -path "${dir}/archived-G-*/streams/rl-default-*" | wc -l)" -eq 2 ]] ||
    fail "the streams aren't in one archive beside the store: $(find "${dir}" | paste -sd' ')"
  [[ "${out}" == *"archived the streams rl-default-desired rl-default-status from ${dir}/jetstream/\$G to ${dir}/archived-G-"* ]] ||
    fail "no log of what it archived: ${out}"
  out="$(sh "${ARCHIVE}" "${dir}/jetstream" "${dir}/auth.conf")"
  [[ "${out}" == "nats-archive: no \$G in ${dir}/jetstream: nothing to archive" ]] || fail "again: ${out}"
  # The same archive name again (a fixed clock): kept, and $G left.
  mkdir -p "${dir}/bin"
  printf '#!/bin/sh\necho 20261113T000000Z\n' >"${dir}/bin/date"
  chmod +x "${dir}/bin/date"
  mkdir -p "${dir}/archived-G-20261113T000000Z/kept"
  g_store "${dir}" rl-default-status
  out="$(PATH="${dir}/bin:${PATH}" sh "${ARCHIVE}" "${dir}/jetstream" "${dir}/auth.conf")"
  [[ -d "${dir}/jetstream/\$G/streams/rl-default-status" && -d "${dir}/archived-G-20261113T000000Z/kept" ]] ||
    fail "an existing archive was overwritten"
  [[ "${out}" == *"archived-G-20261113T000000Z exists: leaving"* ]] || fail "no log of the kept archive: ${out}"
  rm -rf "${dir}"
}

# AC2: anything but Rangelight's streams in $G, or a server whose JetStream isn't RL's, is logged and left.
test_archive_leaves_anything_else() {
  local dir out case
  for case in other-stream other-entry not-rl no-stream; do
    dir="$(mktemp -d /tmp/rl-test-archive.XXXXXX)"
    case "${case}" in
      other-stream) g_store "${dir}" rl-default-desired ORDERS ;;
      other-entry) g_store "${dir}" rl-default-desired && mkdir -p "${dir}/jetstream/\$G/kv" ;;
      not-rl) g_store "${dir}" rl-default-desired && rm -rf "${dir}/jetstream/RL" && : >"${dir}/auth.conf" ;;
      no-stream) g_store "${dir}" && mkdir -p "${dir}/jetstream/\$G/streams" ;;
    esac
    out="$(sh "${ARCHIVE}" "${dir}/jetstream" "${dir}/auth.conf")"
    [[ -d "${dir}/jetstream/\$G" ]] || fail "${case}: \$G was moved"
    [[ "$(find "${dir}" -maxdepth 1 -name 'archived-G-*' | wc -l)" -eq 0 ]] || fail "${case}: archived"
    [[ "${out}" == *"leaving"* ]] || fail "${case}: no log: ${out}"
    rm -rf "${dir}"
  done
}

# Public keys only (no seed): auth.conf needs two valid ones, and nothing signs in to this server.
TEST_ISSUER="ABC4FYBF5QXFIGMPD6SH6PDKW3XWBBJHDGN7R4CAWWCEHWINH6ELO647"
TEST_CALLOUT_USER="UDIQKBXZ5D3HLBDHYZDX3Y4JECECEKDZRNPR5KS7X7HPGDBNLWWNPSQT"

# free_port: a TCP port nothing on 127.0.0.1 listens on.
free_port() {
  local port
  while :; do
    port="$((20000 + RANDOM % 40000))"
    (: <"/dev/tcp/127.0.0.1/${port}") 2>/dev/null || { printf '%s' "${port}"; return 0; }
  done
}

# healthz <port>: the server's full /healthz body, or nothing while it isn't up.
healthz() {
  { exec 4<>"/dev/tcp/127.0.0.1/$1"; } 2>/dev/null || return 0
  printf 'GET /healthz HTTP/1.0\r\n\r\n' >&4
  sed -n '/^{/p' <&4
  exec 4<&-
}

# nats_up <conf> <port> <monitor port>: nats-server with <conf>, once its /healthz answers; its pid in NATS_PID.
nats_up() {
  RL_NATS_ISSUER="${TEST_ISSUER}" RL_NATS_AUTH_USER="${TEST_CALLOUT_USER}" nats-server -c "$1" >>"${1%/*}/server.log" 2>&1 &
  NATS_PID=$!
  local i
  for i in $(seq 100); do
    [[ -z "$(healthz "$3")" ]] || return 0
    sleep 0.1
  done
  fail "nats-server didn't start ($i tries): $(tail -3 "${1%/*}/server.log")"
}

nats_down() {
  kill "${NATS_PID}"
  wait "${NATS_PID}" 2>/dev/null || true
}

# AC1: an install upgraded from before sign-in. The old server made Rangelight's streams in $G; the sign-in config
# can't start on them (the startup probe's full /healthz), the archive step lets it, at every start, with a real
# nats-server and this chart's auth.conf.
test_ac1_an_upgrade_from_before_sign_in_starts_nats_every_time() {
  local dir port monitor stream i body
  command -v nats-server >/dev/null || { fail "no nats-server on PATH (scripts/tools.sh)"; return; }
  dir="$(mktemp -d /tmp/rl-test-nats.XXXXXX)"
  port="$(free_port)"
  monitor="$(free_port)"
  printf 'port: %s\nhttp_port: %s\njetstream { store_dir: "%s/data" }\n' "${port}" "${monitor}" "${dir}" >"${dir}/old.conf"
  nats_up "${dir}/old.conf" "${port}" "${monitor}"
  exec 3<>"/dev/tcp/127.0.0.1/${port}"
  printf 'CONNECT {"verbose":false}\r\n' >&3
  for stream in desired status; do
    body="{\"name\":\"rl-default-${stream}\",\"subjects\":[\"rl.default.${stream}.>\"]}"
    # shellcheck disable=SC2016 # $JS is the NATS subject's own text, not an expansion
    printf 'PUB $JS.API.STREAM.CREATE.rl-default-%s _INBOX.t %s\r\n%s\r\n' "${stream}" "${#body}" "${body}" >&3
  done
  printf 'PING\r\n' >&3
  for i in $(seq 50); do
    [[ -d "${dir}/data/jetstream/\$G/streams/rl-default-status" ]] && break
    sleep 0.1
  done
  exec 3<&-
  nats_down
  [[ -d "${dir}/data/jetstream/\$G/streams/rl-default-desired" ]] || { fail "the old server made no stream"; return; }

  auth_conf >"${dir}/auth.conf"
  printf 'port: %s\nhttp_port: %s\njetstream { store_dir: "%s/data" }\ninclude ./auth.conf\n' "${port}" "${monitor}" "${dir}" \
    >"${dir}/new.conf"
  nats_up "${dir}/new.conf" "${port}" "${monitor}"
  # JSON escapes the stream name's ">" as \u003e.
  [[ "$(healthz "${monitor}")" == *"JetStream stream '\$G \\u003e rl-default-desired' could not be recovered"* ]] ||
    fail "the failure doesn't reproduce: $(healthz "${monitor}")"
  nats_down
  for i in 1 2; do
    sh "${ARCHIVE}" "${dir}/data/jetstream" "${dir}/auth.conf" >/dev/null
    nats_up "${dir}/new.conf" "${port}" "${monitor}"
    [[ "$(healthz "${monitor}")" == '{"status":"ok"}' ]] || fail "start ${i} after the archive: $(healthz "${monitor}")"
    nats_down
  done
  [[ "$(find "${dir}/data" -maxdepth 1 -name 'archived-G-*' | wc -l)" -eq 1 ]] || fail "not one archive"
  rm -rf "${dir}"
}

main "$@"
