#!/usr/bin/env bash
# The dev environment on an existing cluster (RAN-16; ADR 0040, 0042).
# It never creates or deletes a cluster, and never touches the owner-only namespaces.
# Usage: RANGELIGHT_KUBE_CONTEXT=<context> [RANGELIGHT_VALUES=<values file, for up>] scripts/cluster.sh <up|down|preflight|smoke>
# RANGELIGHT_DRY_RUN=true scripts/cluster.sh up prints what up would install, and changes nothing.
set -euo pipefail
shopt -s inherit_errexit

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
COMMANDS=(up down preflight smoke)
SYSTEM_NS=rangelight-system
CHART="${ROOT}/deployments/helm/rangelight"
RELEASE=rangelight
GUARD_POLICY=rangelight-agent-guard
PROBE_TIMEOUT=150
PREFLIGHT_NS=""
FAILS=0
SMOKE_PODS=()

# --- helpers -------------------------------------------------------------------

die() {
  printf 'cluster.sh: %s\n' "$1" >&2
  exit 1
}

# report <PASS|FAIL|INFO> <text>: one line per requirement.
report() {
  printf '%-4s  %s\n' "$1" "$2"
  [[ "$1" != FAIL ]] || FAILS=$((FAILS + 1))
}

# The cluster is always named; there is no default and no fallback to the current context.
require_context() {
  [[ -n "${RANGELIGHT_KUBE_CONTEXT:-}" ]] ||
    die "RANGELIGHT_KUBE_CONTEXT is not set. Set it to the kubeconfig context of an existing cluster; this script never creates one."
  command -v kubectl >/dev/null || die "kubectl is not installed"
  kubectl config get-contexts -o name | grep -qxF -e "${RANGELIGHT_KUBE_CONTEXT}" ||
    die "context '${RANGELIGHT_KUBE_CONTEXT}' (RANGELIGHT_KUBE_CONTEXT) is not in the kubeconfig"
}

k() { kubectl --context "${RANGELIGHT_KUBE_CONTEXT}" "$@"; }
h() { helm --kube-context "${RANGELIGHT_KUBE_CONTEXT}" "$@"; }

load_versions() {
  # shellcheck source=deployments/dev/versions.env
  source "${ROOT}/deployments/dev/versions.env"
}

interrupted() { exit 130; }

# restricted_pod <name> <component> <image> <script> [container-yaml] [pod-yaml]: a pod that passes Pod Security
# `restricted`, with resources set. The extras are appended to the container (keys at 6 spaces), then the spec (2).
restricted_pod() {
  cat <<EOF
---
apiVersion: "v1"
kind: "Pod"
metadata:
  name: "$1"
  labels:
    app.kubernetes.io/name: "$1"
    app.kubernetes.io/component: "$2"
    app.kubernetes.io/part-of: "rangelight"
    app.kubernetes.io/managed-by: "cluster.sh"
spec:
  restartPolicy: "Never"
  activeDeadlineSeconds: 120
  securityContext:
    runAsNonRoot: true
    runAsUser: 65532
    runAsGroup: 65532
    fsGroup: 65532
    seccompProfile:
      type: "RuntimeDefault"
  containers:
    - name: "$2"
      image: "$3"
      command: ["sh", "-c", "$4"]
      resources:
        requests:
          cpu: "50m"
          memory: "64Mi"
        limits:
          memory: "128Mi"
      securityContext:
        allowPrivilegeEscalation: false
        capabilities:
          drop:
            - "ALL"${5:-}${6:-}
EOF
}

# pod_result <namespace> <name>: Succeeded, Failed, or the phase it was stuck in at the deadline.
pod_result() {
  local phase="" _
  for _ in $(seq 1 "${PROBE_TIMEOUT}"); do
    phase="$(k -n "$1" get pod "$2" -o jsonpath='{.status.phase}' 2>/dev/null || true)"
    [[ "${phase}" == Succeeded || "${phase}" == Failed ]] && break
    sleep 1
  done
  printf '%s' "${phase:-Unknown}"
}

# --- up, down, smoke -------------------------------------------------------------

namespace_manifest() {
  cat <<EOF
---
apiVersion: "v1"
kind: "Namespace"
metadata:
  name: "${SYSTEM_NS}"
  labels:
    pod-security.kubernetes.io/enforce: "restricted"
    app.kubernetes.io/part-of: "rangelight"
    app.kubernetes.io/managed-by: "cluster.sh"
EOF
}

up() {
  require_context
  load_versions
  local name; for name in GIT_MIRROR_IMAGE RUNNER_IMAGE AGENT_IMAGE API_IMAGE RELAY_IMAGE WEB_IMAGE; do [[ "${!name}" =~ @sha256:[0-9a-f]{64}$ ]] || die "pin ${name} after the first images.yaml run (deployments/dev/versions.env: ${!name})"; done
  command -v helm >/dev/null || die "helm is not installed"
  [[ -f "${RANGELIGHT_VALUES:-}" ]] || die "RANGELIGHT_VALUES is not a file: your values, at least publicUrl and dex.github.org"
  local dev=(--values "${ROOT}/deployments/dev/values.yaml" --values "${RANGELIGHT_VALUES}" --set-string "gitMirror.image=${GIT_MIRROR_IMAGE}" --set-string "runner.image=${RUNNER_IMAGE}" --set-string "runner.agentImage=${AGENT_IMAGE}" --set-string "api.image=${API_IMAGE}" --set-string "relay.image=${RELAY_IMAGE}" --set-string "web.image=${WEB_IMAGE}")
  if [[ "${RANGELIGHT_DRY_RUN:-}" == true ]]; then
    namespace_manifest
    # The dev cluster has Cilium (the lanes need it): the api reaches the API server through its CiliumNetworkPolicy.
    helm template "${RELEASE}" "${CHART}" --namespace "${SYSTEM_NS}" --api-versions postgresql.cnpg.io/v1/Cluster \
      --api-versions cilium.io/v2 "${dev[@]}"
    return 0
  fi
  namespace_manifest | k apply -f -
  # Twice: the first pass installs the CloudNativePG operator and its CRDs and waits for the operator;
  # the chart renders the Postgres cluster once its CRD exists, so the second pass creates it.
  for _ in 1 2; do h upgrade --install "${RELEASE}" "${CHART}" --namespace "${SYSTEM_NS}" --wait --timeout 10m "${dev[@]}"; done
  if k -n "${SYSTEM_NS}" get clusters.postgresql.cnpg.io rangelight-pg >/dev/null 2>&1; then
    k -n "${SYSTEM_NS}" wait clusters.postgresql.cnpg.io/rangelight-pg --for=condition=Ready --timeout=600s
  fi
  printf 'up: rangelight is installed in %s on context %s\n' "${SYSTEM_NS}" "${RANGELIGHT_KUBE_CONTEXT}"
}

down() {
  require_context
  # The release first: its cluster-scoped objects (the operator's webhooks and roles) outlive the namespace.
  # The CRDs stay (helm.sh/resource-policy: keep), so no other Postgres data goes with them.
  if command -v helm >/dev/null; then
    h uninstall "${RELEASE}" --namespace "${SYSTEM_NS}" --ignore-not-found --wait --timeout 5m
  fi
  k delete namespace "${SYSTEM_NS}" --ignore-not-found --wait=true --timeout=300s
  printf 'down: %s removed (the cluster itself is left alone)\n' "${SYSTEM_NS}"
}

# What the smoke pods run, in their own shell; like the probes below, no double quotes or backslashes.
PSQL_SMOKE="[ \$(psql \$POSTGRES_URL -tAc 'select 1') = 1 ]"
# NATS signs every client in as itself (ADR 0021 amendment): an anonymous one, as this pod is, must be refused.
# shellcheck disable=SC2016
NATS_SMOKE='out=$(nats pub rl.smoke ok 2>&1) && { echo an anonymous client was let in >&2; exit 1; }; case $out in *uthorization*) ;; *) echo $out >&2; exit 1 ;; esac'
# The smoke pods' clients write to their working directory and HOME: a writable emptyDir at /tmp (the root
# filesystem isn't theirs as uid 65532).
SMOKE_TMP=$'\n  volumes:\n    - name: "tmp"\n      emptyDir: {}'

cleanup_smoke() {
  [[ ${#SMOKE_PODS[@]} -gt 0 ]] || return 0
  k -n "${SYSTEM_NS}" delete pod "${SMOKE_PODS[@]}" --ignore-not-found --wait=false >/dev/null 2>&1 ||
    printf 'cluster.sh: could not delete pods %s; delete them by hand\n' "${SMOKE_PODS[*]}" >&2
}

# smoke_container <secret> <key> <nats-url>: the smoke container's /tmp and env; only psql reads the Secret.
smoke_container() {
  printf '\n      workingDir: "/tmp"\n      volumeMounts:\n        - name: "tmp"\n          mountPath: "/tmp"'
  printf '\n      env:\n        - name: "HOME"\n          value: "/tmp"'
  if [[ -n "$1" ]]; then
    printf '\n        - name: "POSTGRES_URL"\n          valueFrom:\n            secretKeyRef:\n              name: "%s"\n              key: "%s"' "$1" "$2"
  else
    printf '\n        - name: "NATS_URL"\n          value: "%s"' "$3"
  fi
}

# check_smoke <pod> <what>: PASS when the pod succeeded, else FAIL with the end of its log.
check_smoke() {
  local result
  result="$(pod_result "${SYSTEM_NS}" "$1")"
  if [[ "${result}" == Succeeded ]]; then
    report PASS "$2"
  else
    report FAIL "$2 (${result}): $(k -n "${SYSTEM_NS}" logs "$1" --tail=3 2>/dev/null | paste -sd' ' || true)"
  fi
}

smoke() {
  require_context
  load_versions
  local settings postgres nats secret key suffix
  k get namespace "${SYSTEM_NS}" >/dev/null 2>&1 || die "smoke: namespace ${SYSTEM_NS} does not exist (run: scripts/cluster.sh up)"
  report PASS "namespace ${SYSTEM_NS} exists"
  # The api's settings, from the chart: the same Postgres and NATS the api uses, bundled or your own.
  settings="$(k -n "${SYSTEM_NS}" get configmap rangelight-api \
    -o jsonpath='{.data.RANGELIGHT_API_POSTGRES_URL} {.data.RANGELIGHT_API_NATS_URL}' 2>/dev/null)" ||
    die "smoke: ConfigMap rangelight-api is missing (run: scripts/cluster.sh up)"
  read -r postgres nats <<<"${settings}"
  [[ "${postgres:-}" =~ ^secret://([a-z0-9][-a-z0-9.]*)#([-._a-zA-Z0-9]+)$ ]] ||
    die "smoke: RANGELIGHT_API_POSTGRES_URL in ConfigMap rangelight-api is not secret://<name>#<key>"
  secret="${BASH_REMATCH[1]}" key="${BASH_REMATCH[2]}"
  [[ "${nats:-}" =~ ^(nats|tls)://[-.a-zA-Z0-9]+(:[0-9]+)?$ ]] ||
    die "smoke: RANGELIGHT_API_NATS_URL in ConfigMap rangelight-api is not a nats:// or tls:// URL"
  suffix="$(od -An -N3 -tx1 /dev/urandom | tr -d ' \n')"
  SMOKE_PODS=("rl-smoke-psql-${suffix}" "rl-smoke-nats-${suffix}")
  trap cleanup_smoke EXIT
  trap interrupted INT TERM
  k -n "${SYSTEM_NS}" apply -f - >/dev/null <<EOF
$(restricted_pod "${SMOKE_PODS[0]}" smoke "${PSQL_IMAGE}" "${PSQL_SMOKE}" "$(smoke_container "${secret}" "${key}" "")" "${SMOKE_TMP}")
$(restricted_pod "${SMOKE_PODS[1]}" smoke "${NATS_BOX_IMAGE}" "${NATS_SMOKE}" "$(smoke_container "" "" "${nats}")" "${SMOKE_TMP}")
EOF
  check_smoke "${SMOKE_PODS[0]}" "psql 'select 1' with the Secret ${secret}, from a restricted pod"
  check_smoke "${SMOKE_PODS[1]}" "NATS refuses an anonymous client on ${nats}, from a restricted pod"
  [[ "${FAILS}" -eq 0 ]] || die "smoke: ${FAILS} check(s) failed"
}

# --- preflight -------------------------------------------------------------------

# shellcheck source=scripts/cluster/preflight.sh
source "${ROOT}/scripts/cluster/preflight.sh"

# --- dispatcher -----------------------------------------------------------------

usage() {
  printf 'usage: RANGELIGHT_KUBE_CONTEXT=<context> scripts/cluster.sh <command>\ncommands: %s\n' "${COMMANDS[*]}" >&2
}

main() {
  local command="${1:-}"
  if [[ " ${COMMANDS[*]} " != *" ${command} "* ]] || ! declare -f "${command}" >/dev/null; then
    usage
    exit 2
  fi
  "${command}"
}

main "$@"
