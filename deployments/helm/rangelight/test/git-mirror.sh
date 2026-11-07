#!/usr/bin/env bash
# What the chart renders for the git mirror (RAN-17), and the values it refuses. Run by `scripts/check.sh deploy`.
# Usage: deployments/helm/rangelight/test/git-mirror.sh [test_name...]   (no argument runs every test)
set -euo pipefail
# shellcheck source=deployments/helm/rangelight/test/lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

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

main "$@"
