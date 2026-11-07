#!/usr/bin/env bash
# The dev environment on an existing cluster (RAN-16; ADR 0040, 0042).
# It never creates or deletes a cluster, and never touches the owner-only namespaces.
# Usage: RANGELIGHT_KUBE_CONTEXT=<context> scripts/cluster.sh <up|down|preflight|smoke>
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
  command -v helm >/dev/null || die "helm is not installed"
  if [[ "${RANGELIGHT_DRY_RUN:-}" == true ]]; then
    namespace_manifest
    helm template "${RELEASE}" "${CHART}" --namespace "${SYSTEM_NS}" --api-versions postgresql.cnpg.io/v1/Cluster
    return 0
  fi
  namespace_manifest | k apply -f -
  # Twice: the first pass installs the CloudNativePG operator and its CRDs and waits for the operator;
  # the chart renders the Postgres cluster once its CRD exists, so the second pass creates it.
  for _ in 1 2; do
    h upgrade --install "${RELEASE}" "${CHART}" --namespace "${SYSTEM_NS}" --wait --timeout 10m
  done
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
# A JetStream round trip: a file-backed stream, one message in, the same message out through a pull consumer.
# It stops at the first failing step, whose error ends the log, and always removes the stream.
# shellcheck disable=SC2016
NATS_SMOKE='set -e; trap '"'"'rc=$?; nats stream rm -f rl-smoke >/dev/null 2>&1 || true; exit $rc'"'"' EXIT; nats stream rm -f rl-smoke >/dev/null 2>&1 || true; nats stream add rl-smoke --subjects rl.smoke --storage file --replicas 1 --defaults >/dev/null; nats pub rl.smoke ok; nats consumer add rl-smoke rl-smoke --pull --defaults >/dev/null; got=$(nats consumer next rl-smoke rl-smoke --raw); [ $got = ok ] || { echo consume: got $got, want ok >&2; exit 1; }'
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
  check_smoke "${SMOKE_PODS[1]}" "JetStream publish and consume on ${nats}, from a restricted pod"
  [[ "${FAILS}" -eq 0 ]] || die "smoke: ${FAILS} check(s) failed"
}

# --- preflight -------------------------------------------------------------------

cleanup_preflight() {
  [[ -n "${PREFLIGHT_NS}" ]] || return 0
  k delete namespace "${PREFLIGHT_NS}" --ignore-not-found --wait=true --timeout=180s >/dev/null 2>&1 ||
    printf 'cluster.sh: could not delete namespace %s; delete it by hand\n' "${PREFLIGHT_NS}" >&2
}

# What the probe pods run, in their own shell. They hold no double quotes or backslashes,
# so they sit as-is in a double-quoted YAML string.
# An egress probe exits 0 as soon as the API server answers at all, else with curl's code:
# 7 (refused) or 28 (timed out) means the connection was blocked.
# shellcheck disable=SC2016
API_PROBE='sleep 5; for i in 1 2 3; do curl -sk -o /dev/null -m 5 https://kubernetes.default.svc/livez && exit 0; rc=$?; sleep 2; done; exit $rc'
# shellcheck disable=SC2016
OIDC_PROBE='sa=/var/run/secrets/kubernetes.io/serviceaccount; curl -sf --cacert $sa/ca.crt --oauth2-bearer $(cat $sa/token) https://kubernetes.default.svc/.well-known/openid-configuration | grep -q jwks_uri && curl -sf --cacert $sa/ca.crt --oauth2-bearer $(cat $sa/token) https://kubernetes.default.svc/openid/v1/jwks | grep -q keys'
PVC_PROBE='echo ok >/data/probe && grep -q ok /data/probe'
PVC_MOUNT=$'\n      volumeMounts:\n        - name: "data"\n          mountPath: "/data"'
PVC_VOLUME=$'\n  volumes:\n    - name: "data"\n      persistentVolumeClaim:\n        claimName: "probe-1gi"'

create_probes() {
  k -n "${PREFLIGHT_NS}" apply -f - >/dev/null <<EOF
---
apiVersion: "networking.k8s.io/v1"
kind: "NetworkPolicy"
metadata:
  name: "deny-egress"
spec:
  podSelector:
    matchLabels:
      app.kubernetes.io/name: "egress-denied"
  policyTypes:
    - "Egress"
  egress:
    - ports:
        - protocol: "UDP"
          port: 53
        - protocol: "TCP"
          port: 53
---
apiVersion: "v1"
kind: "PersistentVolumeClaim"
metadata:
  name: "probe-1gi"
spec:
  accessModes:
    - "ReadWriteOnce"
  resources:
    requests:
      storage: "1Gi"
$(restricted_pod egress-denied preflight "${CURL_IMAGE}" "${API_PROBE}")
$(restricted_pod egress-open preflight "${CURL_IMAGE}" "${API_PROBE}")
$(restricted_pod oidc preflight "${CURL_IMAGE}" "${OIDC_PROBE}")
$(restricted_pod pvc preflight "${CURL_IMAGE}" "${PVC_PROBE}" "${PVC_MOUNT}" "${PVC_VOLUME}")
EOF
}

check_version() {
  local raw major minor want_major want_minor
  raw="$(k get --raw /version)"
  major="$(sed -nE 's/.*"major": *"([0-9]+).*/\1/p' <<<"${raw}")"
  minor="$(sed -nE 's/.*"minor": *"([0-9]+).*/\1/p' <<<"${raw}")"
  want_major="${KUBERNETES_MIN%%.*}" want_minor="${KUBERNETES_MIN#*.}"
  if ((major > want_major || (major == want_major && minor >= want_minor))); then
    report PASS "Kubernetes ${major}.${minor} (>= ${KUBERNETES_MIN})"
  else
    report FAIL "Kubernetes ${major:-?}.${minor:-?}, want >= ${KUBERNETES_MIN}"
  fi
}

check_psa() {
  local out
  if out="$(k -n "${PREFLIGHT_NS}" run psa-privileged --image="${CURL_IMAGE}" --restart=Never --dry-run=server \
    --overrides='{"spec":{"containers":[{"name":"psa-privileged","image":"'"${CURL_IMAGE}"'","securityContext":{"privileged":true}}]}}' 2>&1)"; then
    report FAIL "Pod Security admitted a privileged pod in a restricted namespace"
  elif [[ "${out}" == *PodSecurity* ]]; then
    report PASS "Pod Security rejects a privileged pod (restricted)"
  else
    report FAIL "privileged pod refused, but not by Pod Security: ${out}"
  fi
}

check_egress() {
  local denied open code
  denied="$(pod_result "${PREFLIGHT_NS}" egress-denied)"
  open="$(pod_result "${PREFLIGHT_NS}" egress-open)"
  code="$(k -n "${PREFLIGHT_NS}" get pod egress-denied \
    -o jsonpath='{.status.containerStatuses[0].state.terminated.exitCode}' 2>/dev/null || true)"
  if [[ "${open}" != Succeeded ]]; then
    report FAIL "egress NetworkPolicy: the control pod could not reach kubernetes.default.svc (${open}), inconclusive"
  elif [[ "${denied}" == Succeeded ]]; then
    report FAIL "egress NetworkPolicy not enforced: a deny-egress pod reached kubernetes.default.svc"
  elif [[ "${code}" != 7 && "${code}" != 28 ]]; then
    report FAIL "egress NetworkPolicy: the deny-egress pod ended ${denied} (curl exit ${code:-none}), not a blocked connection"
  else
    report PASS "egress NetworkPolicy enforced: the deny-egress pod can't reach kubernetes.default.svc, the open pod can"
  fi
}

check_oidc() {
  local result
  result="$(pod_result "${PREFLIGHT_NS}" oidc)"
  if [[ "${result}" == Succeeded ]]; then
    report PASS "OIDC discovery and JWKS reachable from a pod"
  else
    report FAIL "OIDC discovery not reachable from a pod (${result})"
  fi
}

check_pvc() {
  local result phase
  result="$(pod_result "${PREFLIGHT_NS}" pvc)"
  phase="$(k -n "${PREFLIGHT_NS}" get pvc probe-1gi -o jsonpath='{.status.phase}')"
  if [[ "${phase}" == Bound && "${result}" == Succeeded ]]; then
    report PASS "a 1 GiB PVC binds (default StorageClass) and is writable"
  else
    report FAIL "a 1 GiB PVC: ${phase:-no phase}, writer pod ${result}"
  fi
}

info_gvisor() {
  local classes
  classes="$(k get runtimeclass -o jsonpath='{range .items[*]}{.metadata.name}={.handler}{"\n"}{end}' |
    grep -E '=(runsc|gvisor)|^gvisor=' | cut -d= -f1 | paste -sd, || true)"
  report INFO "gVisor RuntimeClass: ${classes:-none (optional, ADR 0015)}"
}

info_guard() {
  if k get validatingadmissionpolicy "${GUARD_POLICY}" >/dev/null 2>&1; then
    report INFO "admission policy ${GUARD_POLICY}: present"
  else
    report INFO "admission policy ${GUARD_POLICY}: absent"
  fi
}

preflight() {
  require_context
  load_versions
  check_version
  PREFLIGHT_NS="rangelight-preflight-$(od -An -N3 -tx1 /dev/urandom | tr -d ' \n')"
  trap cleanup_preflight EXIT
  trap interrupted INT TERM
  k create -f - >/dev/null <<EOF
---
apiVersion: "v1"
kind: "Namespace"
metadata:
  name: "${PREFLIGHT_NS}"
  labels:
    pod-security.kubernetes.io/enforce: "restricted"
    app.kubernetes.io/part-of: "rangelight"
    app.kubernetes.io/component: "preflight"
EOF
  # Pods are refused until the namespace's default ServiceAccount exists.
  for _ in $(seq 1 30); do k -n "${PREFLIGHT_NS}" get serviceaccount default >/dev/null 2>&1 && break; sleep 1; done
  create_probes
  check_egress
  check_psa
  check_oidc
  check_pvc
  info_gvisor
  info_guard
  [[ "${FAILS}" -eq 0 ]] || die "preflight: ${FAILS} requirement(s) failed"
}

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
