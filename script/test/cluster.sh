#!/usr/bin/env bash
# Self-tests for script/cluster.sh that need no cluster: kubectl is a stub that records its calls.
# Each test copies cluster.sh and deployments/dev/ into /tmp/rl-cluster-test.*.
# Usage: script/test/cluster.sh [test_name...]   (no argument runs every test)
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel)"
DIR=""
OUT=""
STATUS=0
FAILED=0

new_dir() {
  DIR="$(mktemp -d /tmp/rl-cluster-test.XXXXXX)"
  mkdir -p "${DIR}/script" "${DIR}/deployments/dev" "${DIR}/bin"
  cp "${ROOT}/script/cluster.sh" "${DIR}/script/"
  cp "${ROOT}/deployments/dev/versions.env" "${ROOT}/deployments/dev/zot.yaml" "${DIR}/deployments/dev/"
  : >"${DIR}/calls"
  # The stub knows one context, "dev", records every call, keeps what `apply` reads,
  # and reports that no namespace exists.
  cat >"${DIR}/bin/kubectl" <<EOF
#!/usr/bin/env bash
printf '%s\n' "\$*" >>"${DIR}/calls"
case "\$*" in
  "config get-contexts -o name") printf 'dev\n' ;;
  *" apply -f -"*) cat >"${DIR}/applied" ;;
  *" get namespace "*) exit 1 ;;
esac
EOF
  chmod +x "${DIR}/bin/kubectl"
}

# run <command> [env assignments...]: run cluster.sh with the stub kubectl first on PATH.
run() {
  local command="$1"
  shift
  STATUS=0
  OUT="$(env -u RANGELIGHT_KUBE_CONTEXT PATH="${DIR}/bin:${PATH}" "$@" \
    bash "${DIR}/script/cluster.sh" ${command:+"${command}"} 2>&1)" || STATUS=$?
}

fail() {
  printf '  FAIL: %s\n  --- output ---\n%s\n  --------------\n' "$1" "${OUT}"
  FAILED=1
}

expect_status() { [[ "${STATUS}" -eq "$1" ]] || fail "expected exit $1, got ${STATUS}"; }
expect_out() { [[ "${OUT}" == *"$1"* ]] || fail "expected output to contain: $1"; }
expect_no_calls() { [[ ! -s "${DIR}/calls" ]] || fail "kubectl was called: $(<"${DIR}/calls")"; }
expect_no_call() { ! grep -qE -e "$1" "${DIR}/calls" || fail "unexpected kubectl call matching: $1"; }

# --- dispatcher ----------------------------------------------------------------

test_no_command_prints_usage() {
  run ""
  expect_status 2
  expect_out "usage:"
  expect_no_calls
}

test_unknown_command_prints_usage() {
  run create-cluster RANGELIGHT_KUBE_CONTEXT=dev
  expect_status 2
  expect_out "usage:"
  expect_no_calls
}

# --- AC3: the context is required, and nothing runs without it -------------------

test_every_command_fails_fast_without_context() {
  local command
  for command in up down preflight smoke; do
    run "${command}"
    expect_status 1
    expect_out "RANGELIGHT_KUBE_CONTEXT is not set"
  done
  expect_no_calls
}

test_unknown_context_fails_before_any_change() {
  run up RANGELIGHT_KUBE_CONTEXT=elsewhere
  expect_status 1
  expect_out "context 'elsewhere' (RANGELIGHT_KUBE_CONTEXT) is not in the kubeconfig"
  [[ "$(<"${DIR}/calls")" == "config get-contexts -o name" ]] || fail "kubectl did more than list contexts"
}

test_every_cluster_call_names_the_context() {
  run up RANGELIGHT_KUBE_CONTEXT=dev
  expect_status 0
  ! grep -vE -e '^config get-contexts -o name$' -e '^--context dev ' "${DIR}/calls" ||
    fail "a kubectl call without --context dev"
}

test_up_never_deletes_or_makes_a_cluster() {
  run up RANGELIGHT_KUBE_CONTEXT=dev
  expect_status 0
  expect_no_call ' delete '
  ! grep -qE -e '\b(k3d|minikube|eksctl)\b|\bkind (create|delete)\b|cluster (create|delete)' "${DIR}/script/cluster.sh" ||
    fail "cluster.sh can create or delete a cluster"
}

# --- AC6: up stays out of the owner-only namespaces --------------------------------

test_up_applies_only_rangelight_system() {
  run up RANGELIGHT_KUBE_CONTEXT=dev
  expect_status 0
  [[ -s "${DIR}/applied" ]] || fail "up applied nothing"
  ! grep -E -e '^\s*namespace:' "${DIR}/applied" | grep -vqE -e 'namespace: "rangelight-system"$' ||
    fail "up applies into a namespace other than rangelight-system"
  ! grep -qE -e 'rangelight-(gateway|runner)' "${DIR}/applied" "${DIR}/calls" || fail "up names an owner-only namespace"
}

test_up_refuses_a_manifest_naming_an_owner_only_namespace() {
  # shellcheck disable=SC2016 # a literal placeholder
  sed -i 's/namespace: "\${SYSTEM_NS}"/namespace: "rangelight-gateway"/' "${DIR}/deployments/dev/zot.yaml"
  run up RANGELIGHT_KUBE_CONTEXT=dev
  expect_status 1
  expect_out "owner-only namespace"
  expect_no_call ' apply '
}

# --- AC5: pinned versions --------------------------------------------------------

test_up_applies_the_pinned_zot_image() {
  run up RANGELIGHT_KUBE_CONTEXT=dev
  local image
  image="$(sed -n 's/^ZOT_IMAGE=//p' "${DIR}/deployments/dev/versions.env")"
  grep -qF -e "image: \"${image}\"" "${DIR}/applied" || fail "the applied Zot image is not ${image}"
  # shellcheck disable=SC2016 # a literal placeholder
  ! grep -qF -e '${' "${DIR}/applied" || fail "an unfilled placeholder was applied"
}

test_images_are_pinned_by_tag_and_digest() {
  local line bad=0
  while IFS= read -r line; do
    [[ "${line}" =~ ^[A-Z_]+_IMAGE=[^:@]+(/[^:@]+)*:[^@:]+@sha256:[0-9a-f]{64}$ ]] || {
      OUT="${line}"
      bad=1
    }
  done < <(grep -E '^[A-Z_]+_IMAGE=' "${DIR}/deployments/dev/versions.env")
  [[ "${bad}" -eq 0 ]] || fail "an image is not pinned by tag and digest"
  OUT="$(grep -rniE -e '[:= ]latest\b' "${DIR}/deployments/dev" "${DIR}/script/cluster.sh" || true)"
  [[ -z "${OUT}" ]] || fail "a 'latest' reference"
  OUT="$(grep -nE -e 'image: "?[^"$]' "${DIR}/deployments/dev/zot.yaml" || true)"
  [[ -z "${OUT}" ]] || fail "zot.yaml hard-codes an image instead of using versions.env"
}

# --- AC2: smoke fails before install ----------------------------------------------

test_smoke_fails_without_rangelight_system() {
  run smoke RANGELIGHT_KUBE_CONTEXT=dev
  expect_status 1
  expect_out "FAIL  namespace rangelight-system does not exist"
  expect_out "FAIL  Zot /v2/"
}

# --- runner -----------------------------------------------------------------------

main() {
  local tests=() t
  if [[ $# -gt 0 ]]; then tests=("$@"); else mapfile -t tests < <(declare -F | awk '$3 ~ /^test_/ { print $3 }'); fi
  for t in "${tests[@]}"; do
    new_dir
    printf '%s\n' "${t}"
    "${t}"
    rm -rf "${DIR}"
  done
  [[ "${FAILED}" -eq 0 ]] || { printf 'cluster self-tests: FAILED\n' >&2; exit 1; }
  printf 'cluster self-tests: ok\n'
}

main "$@"
