#!/usr/bin/env bash
# Self-tests for scripts/cluster.sh that need no cluster: kubectl and helm are stubs that record their calls
# (helm template runs the real helm). Each test copies cluster.sh, deployments/dev/ and the chart into
# /tmp/rl-cluster-test.*.
# Usage: scripts/test/cluster.sh [test_name...]   (no argument runs every test)
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel)"
DIR=""
OUT=""
STATUS=0
FAILED=0

PINNED_MIRROR="ghcr.io/rangelight/git-mirror:abc@sha256:$(printf '1%.0s' $(seq 1 64))"

new_dir() {
  DIR="$(mktemp -d /tmp/rl-cluster-test.XXXXXX)"
  mkdir -p "${DIR}/scripts" "${DIR}/deployments/dev" "${DIR}/deployments/helm" "${DIR}/bin"
  cp "${ROOT}/scripts/cluster.sh" "${DIR}/scripts/"
  cp "${ROOT}/deployments/dev/versions.env" "${ROOT}/deployments/dev/values.yaml" "${DIR}/deployments/dev/"
  # A pinned git mirror image, as after the first images.yaml run.
  sed -i "s|^GIT_MIRROR_IMAGE=.*|GIT_MIRROR_IMAGE=${PINNED_MIRROR}|" "${DIR}/deployments/dev/versions.env"
  cp -R "${ROOT}/deployments/helm/rangelight" "${DIR}/deployments/helm/"
  : >"${DIR}/calls"
  # The kubectl stub knows one context, "dev", records every call, keeps what `apply` reads, reports that
  # no namespace exists, serves the chart's ConfigMap rangelight-api, and says every pod Succeeded.
  cat >"${DIR}/bin/kubectl" <<EOF
#!/usr/bin/env bash
printf '%s\n' "\$*" >>"${DIR}/calls"
case "\$*" in
  "config get-contexts -o name") printf 'dev\n' ;;
  *" apply -f -"*) cat >>"${DIR}/applied" ;;
  *" get namespace "*) exit 1 ;;
  *" get configmap rangelight-api "*) printf 'secret://rangelight-pg-app#uri nats://rangelight-nats.rangelight-system.svc:4222' ;;
  *" get pod "*) printf 'Succeeded' ;;
  *" logs "*) printf 'nats: error: no responders\n' ;;
esac
EOF
  # The helm stub records every call and runs the real helm only for `template`, which needs no cluster.
  cat >"${DIR}/bin/helm" <<EOF
#!/usr/bin/env bash
printf 'helm %s\n' "\$*" >>"${DIR}/calls"
[[ "\$1" != template ]] || exec "$(command -v helm)" "\$@"
EOF
  chmod +x "${DIR}/bin/kubectl" "${DIR}/bin/helm"
}

# run <command> [env assignments...]: run cluster.sh with the stub kubectl first on PATH.
run() {
  local command="$1"
  shift
  STATUS=0
  OUT="$(env -u RANGELIGHT_KUBE_CONTEXT PATH="${DIR}/bin:${PATH}" "$@" \
    bash "${DIR}/scripts/cluster.sh" ${command:+"${command}"} 2>&1)" || STATUS=$?
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
  ! grep -vE -e '^config get-contexts -o name$' -e '^--context dev ' -e '^helm --kube-context dev ' "${DIR}/calls" ||
    fail "a kubectl or helm call without the context dev"
}

test_up_never_deletes_or_makes_a_cluster() {
  run up RANGELIGHT_KUBE_CONTEXT=dev
  expect_status 0
  expect_no_call ' delete '
  ! grep -qE -e '\b(k3d|minikube|eksctl)\b|\bkind (create|delete)\b|cluster (create|delete)' "${DIR}/scripts/cluster.sh" ||
    fail "cluster.sh can create or delete a cluster"
}

# --- AC6: up stays out of the owner-only namespaces --------------------------------

test_up_applies_only_rangelight_system() {
  run up RANGELIGHT_KUBE_CONTEXT=dev
  expect_status 0
  grep -qxF -e '  name: "rangelight-system"' "${DIR}/applied" || fail "up did not apply rangelight-system"
  ! grep -E -e '^\s*namespace:' "${DIR}/applied" | grep -vqE -e 'namespace: "rangelight-system"$' ||
    fail "up applies into a namespace other than rangelight-system"
  ! grep -qE -e 'rangelight-(gateway|runner)' "${DIR}/applied" "${DIR}/calls" || fail "up names an owner-only namespace"
}

# --- AC5: pinned versions --------------------------------------------------------

test_images_are_pinned_by_tag_and_digest() {
  local line bad=0
  while IFS= read -r line; do
    [[ "${line}" =~ ^[A-Z_]+_IMAGE=[^:@]+(/[^:@]+)*:[^@:]+@sha256:[0-9a-f]{64}$ ]] || {
      OUT="${line}"
      bad=1
    }
  done < <(grep -E '^[A-Z_]+_IMAGE=' "${DIR}/deployments/dev/versions.env")
  [[ "${bad}" -eq 0 ]] || fail "an image is not pinned by tag and digest"
  OUT="$(grep -rniE -e '[:= ]latest\b' "${DIR}/deployments/dev" "${DIR}/scripts/cluster.sh" || true)"
  [[ -z "${OUT}" ]] || fail "a 'latest' reference"
}

# --- AC2: smoke fails before install ----------------------------------------------

test_smoke_fails_without_rangelight_system() {
  run smoke RANGELIGHT_KUBE_CONTEXT=dev
  expect_status 1
  expect_out "namespace rangelight-system does not exist"
  expect_no_call ' apply '
}

# --- RAN-33 AC4: up installs the chart, smoke proves Postgres and JetStream ----------

test_up_installs_the_chart_twice_into_rangelight_system() {
  run up RANGELIGHT_KUBE_CONTEXT=dev
  expect_status 0
  [[ "$(grep -c '^helm --kube-context dev upgrade --install rangelight .*/deployments/helm/rangelight --namespace rangelight-system --wait ' "${DIR}/calls")" -eq 2 ]] ||
    fail "up did not run helm upgrade --install twice"
  [[ "$(grep -n ' apply -f -' "${DIR}/calls" | cut -d: -f1)" -lt "$(grep -n '^helm ' "${DIR}/calls" | head -1 | cut -d: -f1)" ]] ||
    fail "up installed the chart before the namespace"
}

# --- RAN-17: the git mirror, with its pinned image -------------------------------------

test_up_installs_the_git_mirror_with_the_pinned_image() {
  run up RANGELIGHT_KUBE_CONTEXT=dev
  expect_status 0
  [[ "$(grep -cF -e "--values ${DIR}/deployments/dev/values.yaml --set-string gitMirror.image=${PINNED_MIRROR}" "${DIR}/calls")" -eq 2 ]] ||
    fail "up did not pass the dev values and the pinned git mirror image to both passes"
  run up RANGELIGHT_KUBE_CONTEXT=dev RANGELIGHT_DRY_RUN=true
  expect_status 0
  expect_out "image: \"${PINNED_MIRROR}\""
}

test_up_refuses_an_unpinned_git_mirror_image() {
  sed -i 's|^GIT_MIRROR_IMAGE=.*|GIT_MIRROR_IMAGE=UNPINNED|' "${DIR}/deployments/dev/versions.env"
  run up RANGELIGHT_KUBE_CONTEXT=dev
  expect_status 1
  expect_out "pin GIT_MIRROR_IMAGE after the first images.yaml run"
  [[ "$(<"${DIR}/calls")" == "config get-contexts -o name" ]] || fail "up called the cluster before refusing"
}

test_the_checked_in_git_mirror_image_is_pinned_or_the_placeholder() {
  grep -qxE -e "GIT_MIRROR_IMAGE=(UNPINNED|[^@[:space:]]+:[^@[:space:]]+@sha256:[0-9a-f]{64})" "${ROOT}/deployments/dev/versions.env" ||
    fail "deployments/dev/versions.env: GIT_MIRROR_IMAGE is neither pinned nor UNPINNED"
}

test_up_dry_run_changes_nothing_and_prints_the_install() {
  run up RANGELIGHT_KUBE_CONTEXT=dev RANGELIGHT_DRY_RUN=true
  expect_status 0
  [[ "$(<"${DIR}/calls")" == $'config get-contexts -o name\nhelm template '* ]] || fail "dry run called the cluster: $(<"${DIR}/calls")"
  expect_out 'pod-security.kubernetes.io/enforce: "restricted"'
  expect_out 'kind: "Cluster"'
  expect_out 'owner: "rl_api"'
}

test_smoke_runs_psql_and_jetstream_from_restricted_pods() {
  sed -i '/ get namespace /d' "${DIR}/bin/kubectl"
  run smoke RANGELIGHT_KUBE_CONTEXT=dev
  expect_status 0
  expect_out "PASS  namespace rangelight-system exists"
  expect_out "PASS  psql 'select 1' with the Secret rangelight-pg-app"
  expect_out "PASS  JetStream publish and consume on nats://rangelight-nats.rangelight-system.svc:4222"
  [[ "$(grep -c '^kind: "Pod"$' "${DIR}/applied")" -eq 2 ]] || fail "smoke did not create two pods"
  grep -qF -e "command: [\"sh\", \"-c\", \"[ \$(psql \$POSTGRES_URL -tAc 'select 1') = 1 ]\"]" "${DIR}/applied" ||
    fail "the psql pod does not run select 1"
  grep -qxF -e '              name: "rangelight-pg-app"' "${DIR}/applied" || fail "psql does not read the CNPG Secret"
  grep -qxF -e '          value: "nats://rangelight-nats.rangelight-system.svc:4222"' "${DIR}/applied" || fail "nats pod without NATS_URL"
  [[ "$(grep -cxF -e '    app.kubernetes.io/part-of: "rangelight"' "${DIR}/applied")" -eq 2 ]] ||
    fail "a smoke pod without the part-of label the NetworkPolicies allow"
  [[ "$(grep -cxF -e '    runAsNonRoot: true' "${DIR}/applied")" -eq 2 ]] || fail "a smoke pod that is not restricted"
  # The clients run as uid 65532: both need a writable working directory and HOME (the live failure: stat .).
  [[ "$(grep -cxF -e '      workingDir: "/tmp"' "${DIR}/applied")" -eq 2 ]] || fail "a smoke pod without workingDir /tmp"
  [[ "$(grep -xF -A1 -e '        - name: "HOME"' "${DIR}/applied" | grep -cxF -e '          value: "/tmp"')" -eq 2 ]] ||
    fail "a smoke pod without HOME=/tmp"
  [[ "$(grep -xF -A1 -e '        - name: "tmp"' "${DIR}/applied" | grep -cxF -e '          mountPath: "/tmp"')" -eq 2 ]] ||
    fail "a smoke pod without /tmp mounted"
  [[ "$(grep -xF -A1 -e '    - name: "tmp"' "${DIR}/applied" | grep -cxF -e '      emptyDir: {}')" -eq 2 ]] ||
    fail "a smoke pod whose /tmp is not an emptyDir"
  grep -qF -e 'command: ["sh", "-c", "set -e; trap ' "${DIR}/applied" || fail "the nats script does not stop at the first failing step"
  grep -qE -e '^--context dev -n rangelight-system delete pod rl-smoke-psql-[0-9a-f]+ rl-smoke-nats-[0-9a-f]+ ' "${DIR}/calls" ||
    fail "smoke did not delete its pods"
}

test_smoke_fails_when_a_pod_fails() {
  sed -i -e '/ get namespace /d' -e "s/printf 'Succeeded'/printf 'Failed'/" "${DIR}/bin/kubectl"
  run smoke RANGELIGHT_KUBE_CONTEXT=dev
  expect_status 1
  expect_out "FAIL  JetStream publish and consume"
  expect_out "nats: error: no responders"
}

test_smoke_refuses_a_settings_value_it_would_put_in_yaml() {
  sed -i -e '/ get namespace /d' -e 's|nats://rangelight-nats|nats://x\"y|' "${DIR}/bin/kubectl"
  run smoke RANGELIGHT_KUBE_CONTEXT=dev
  expect_status 1
  expect_out "RANGELIGHT_API_NATS_URL in ConfigMap rangelight-api is not a nats:// or tls:// URL"
  expect_no_call ' apply '
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
