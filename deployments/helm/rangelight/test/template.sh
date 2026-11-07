#!/usr/bin/env bash
# What the chart renders in each mode, and the values it refuses. Run by `scripts/check.sh deploy`.
# Usage: deployments/helm/rangelight/test/template.sh [test_name...]   (no argument runs every test)
set -euo pipefail

CHART="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CNPG_API="postgresql.cnpg.io/v1/Cluster"
EXTERNAL=(--set postgres.enabled=false --set postgres.external.secretRef.name=pg-api
  --set nats.enabled=false --set nats.external.url=tls://nats.example.com:4222)
OUT=""
STATUS=0
FAILED=0

# render [helm template flags...]: the chart in rangelight-system, into OUT and STATUS.
render() {
  STATUS=0
  OUT="$(helm template rangelight "${CHART}" --namespace rangelight-system "$@" 2>&1)" || STATUS=$?
}

fail() {
  printf '  FAIL: %s\n' "$1"
  FAILED=1
}

expect_ok() { [[ "${STATUS}" -eq 0 ]] || { fail "render failed"; printf '%s\n' "${OUT}"; }; }
expect_refused() { [[ "${STATUS}" -ne 0 && "${OUT}" == *"$1"* ]] || fail "expected the render to fail with: $1"; }
expect_line() { grep -qxF -e "$1" <<<"${OUT}" || fail "expected the line: $1"; }
expect_no_match() { ! grep -qE -e "$1" <<<"${OUT}" || fail "unexpected: $(grep -E -e "$1" <<<"${OUT}" | head -1)"; }

# kinds: every object as Kind/name, sorted.
kinds() {
  awk '/^---/ { k = "" } /^kind:/ { k = $2 } /^  name:/ && k != "" { print k "/" $2; k = "" }' <<<"${OUT}" | tr -d '"' | sort
}

# expect_restricted: the rendered pod passes Pod Security `restricted` and has resources set.
expect_restricted() {
  local want
  for want in "runAsNonRoot: true" "allowPrivilegeEscalation: false" "- ALL" "type: RuntimeDefault" "limits:" "requests:"; do
    grep -qE -e "^ +${want}$" <<<"${OUT}" || fail "pod without '${want}'"
  done
}

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

# --- the git mirror (RAN-17) -------------------------------------------------------

GIT_MIRROR=(-f "${CHART}/test/git-mirror-values.yaml")
MIRROR_IMAGE="ghcr.io/rangelight/git-mirror:test@sha256:$(printf '1%.0s' {1..64})"

# mirror: all of the git mirror's templates, switched on.
mirror() {
  local t args=()
  for t in deployment service pvc networkpolicy; do args+=(--show-only "templates/git-mirror/${t}.yaml"); done
  render "${GIT_MIRROR[@]}" "${args[@]}"
}

# count_lines <line>: how many rendered lines are exactly that.
count_lines() { grep -cxF -e "$1" <<<"${OUT}" || true; }

test_git_mirror_is_off_by_default() {
  render --api-versions "${CNPG_API}"
  expect_ok
  expect_no_match 'git-mirror'
}

test_git_mirror_renders_its_objects_restricted_and_labelled() {
  local label
  mirror
  expect_ok
  [[ "$(kinds | paste -sd' ')" == "Deployment/git-mirror NetworkPolicy/git-mirror PersistentVolumeClaim/git-mirror-data Service/git-mirror" ]] ||
    fail "the git mirror renders: $(kinds | paste -sd' ')"
  [[ "$(grep -cxF -e "          image: \"${MIRROR_IMAGE}\"" <<<"${OUT}")" -eq 4 ]] || fail "not every container runs the pinned image"
  expect_line '        runAsNonRoot: true'
  expect_line '          type: "RuntimeDefault"'
  for label in '            allowPrivilegeEscalation: false' '            readOnlyRootFilesystem: true' '                - "ALL"' '            limits:'; do
    [[ "$(count_lines "${label}")" -eq 4 ]] || fail "a container without '${label# *}'"
  done
  # app.kubernetes.io/name is also in the Service's selector, so it is counted through the others.
  for label in component part-of managed-by; do
    [[ "$(grep -cE -e "^    app.kubernetes.io/${label}: \"" <<<"${OUT}")" -eq 4 ]] || fail "an object without app.kubernetes.io/${label}"
  done
}

test_git_mirror_passes_repos_as_arguments_not_shell() {
  mirror
  expect_ok
  [[ "$(count_lines '            - "opentelemetry-demo=https://github.com/rangelight/opentelemetry-demo.git"')" -eq 2 ]] ||
    fail "the clone and the sync don't both get the repo as an argument"
  expect_no_match '"sh".*github|-c.*opentelemetry'
}

test_git_mirror_nginx_has_its_own_tmp() {
  render "${GIT_MIRROR[@]}" --show-only templates/git-mirror/deployment.yaml
  expect_ok
  expect_line '            - name: "nginx-tmp"'
  [[ "$(count_lines '            - name: "tmp"')" -eq 2 ]] || fail "only the clone and the sync share /tmp"
}

test_git_mirror_ingress_only_from_lanes_and_the_runner() {
  render "${GIT_MIRROR[@]}" --show-only templates/git-mirror/networkpolicy.yaml
  expect_ok
  expect_line '              rangelight.dev/lane: "true"'
  expect_line '              kubernetes.io/metadata.name: "rangelight-runner"'
  expect_line '          port: 8080'
  expect_no_match 'ipBlock|podSelector: \{\}'
}

test_refuses_git_mirror_without_an_image() {
  render --set gitMirror.enabled=true --set 'gitMirror.repos[0].name=demo' --set 'gitMirror.repos[0].upstream=https://example.com/demo'
  expect_refused "/gitMirror/image"
}

test_refuses_git_mirror_image_without_a_digest() {
  render "${GIT_MIRROR[@]}" --set gitMirror.image=ghcr.io/rangelight/git-mirror:test
  expect_refused "/gitMirror/image"
}

test_refuses_git_mirror_without_repos() {
  render --set gitMirror.enabled=true --set "gitMirror.image=${MIRROR_IMAGE}"
  expect_refused "/gitMirror/repos"
}

test_refuses_a_git_mirror_upstream_with_shell_in_it() {
  # shellcheck disable=SC2016 # the literal $(id) is the point: it must never reach a shell
  render "${GIT_MIRROR[@]}" --set 'gitMirror.repos[0].upstream=https://example.com/$(id)'
  expect_refused "/gitMirror/repos/0/upstream"
}

test_refuses_a_git_mirror_repo_name_ending_in_git() {
  render "${GIT_MIRROR[@]}" --set 'gitMirror.repos[0].name=demo.git'
  expect_refused "/gitMirror/repos/0/name"
}

# --- runner -----------------------------------------------------------------------

main() {
  local tests=() t
  if [[ $# -gt 0 ]]; then tests=("$@"); else mapfile -t tests < <(declare -F | awk '$3 ~ /^test_/ { print $3 }'); fi
  for t in "${tests[@]}"; do
    printf '%s\n' "${t}"
    "${t}"
  done
  [[ "${FAILED}" -eq 0 ]] || { printf 'chart template tests: FAILED\n' >&2; exit 1; }
  printf 'chart template tests: ok\n'
}

main "$@"
