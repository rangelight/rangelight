#!/usr/bin/env bash
# The dev environment on an existing cluster (RAN-16; ADR 0040, 0042).
# It never creates or deletes a cluster, and never touches the owner-only namespaces.
# Usage: RANGELIGHT_KUBE_CONTEXT=<context> scripts/cluster.sh <up|down|preflight|smoke>
set -euo pipefail
shopt -s inherit_errexit

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
COMMANDS=(up down preflight smoke)
SYSTEM_NS=rangelight-system
GUARD_POLICY=rangelight-agent-guard
PROBE_TIMEOUT=150
PREFLIGHT_NS=""
FAILS=0

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

load_versions() {
  # shellcheck source=deployments/dev/versions.env
  source "${ROOT}/deployments/dev/versions.env"
}

# --- up, down, smoke -------------------------------------------------------------

up() {
  require_context
  k apply -f - <<EOF
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
  printf 'up: %s is ready on context %s\n' "${SYSTEM_NS}" "${RANGELIGHT_KUBE_CONTEXT}"
}

down() {
  require_context
  k delete namespace "${SYSTEM_NS}" --ignore-not-found --wait=true --timeout=300s
  printf 'down: %s removed (the cluster itself is left alone)\n' "${SYSTEM_NS}"
}

smoke() {
  require_context
  if k get namespace "${SYSTEM_NS}" >/dev/null 2>&1; then
    report PASS "namespace ${SYSTEM_NS} exists"
  else
    report FAIL "namespace ${SYSTEM_NS} does not exist (run: scripts/cluster.sh up)"
  fi
  [[ "${FAILS}" -eq 0 ]] || die "smoke: ${FAILS} check(s) failed"
}

# --- preflight -------------------------------------------------------------------

interrupted() { exit 130; }

cleanup_preflight() {
  [[ -n "${PREFLIGHT_NS}" ]] || return 0
  k delete namespace "${PREFLIGHT_NS}" --ignore-not-found --wait=true --timeout=180s >/dev/null 2>&1 ||
    printf 'cluster.sh: could not delete namespace %s; delete it by hand\n' "${PREFLIGHT_NS}" >&2
}

# A pod spec that passes Pod Security `restricted`. probe_pod <name> <probe-label> <script> [claim]
probe_pod() {
  local volumes="" mounts=""
  if [[ -n "${4:-}" ]]; then
    mounts=$'\n      volumeMounts:\n        - name: "data"\n          mountPath: "/data"'
    volumes=$'\n  volumes:\n    - name: "data"\n      persistentVolumeClaim:\n        claimName: "'"$4"'"'
  fi
  cat <<EOF
---
apiVersion: "v1"
kind: "Pod"
metadata:
  name: "$1"
  labels:
    app.kubernetes.io/part-of: "rangelight"
    app.kubernetes.io/component: "preflight"
    rangelight.dev/probe: "$2"
spec:
  restartPolicy: "Never"
  activeDeadlineSeconds: 120
  securityContext:
    runAsNonRoot: true
    runAsUser: 100
    runAsGroup: 101
    fsGroup: 101
    seccompProfile:
      type: "RuntimeDefault"
  containers:
    - name: "probe"
      image: "${CURL_IMAGE}"
      command: ["sh", "-c", "$3"]
      securityContext:
        allowPrivilegeEscalation: false
        capabilities:
          drop:
            - "ALL"${mounts}${volumes}
EOF
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
      rangelight.dev/probe: "egress-denied"
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
$(probe_pod egress-denied egress-denied "${API_PROBE}")
$(probe_pod egress-open egress-open "${API_PROBE}")
$(probe_pod oidc oidc "${OIDC_PROBE}")
$(probe_pod pvc pvc "${PVC_PROBE}" probe-1gi)
EOF
}

# pod_result <name>: Succeeded, Failed, or the phase it was stuck in at the deadline.
pod_result() {
  local phase="" _
  for _ in $(seq 1 "${PROBE_TIMEOUT}"); do
    phase="$(k -n "${PREFLIGHT_NS}" get pod "$1" -o jsonpath='{.status.phase}' 2>/dev/null || true)"
    [[ "${phase}" == Succeeded || "${phase}" == Failed ]] && break
    sleep 1
  done
  printf '%s' "${phase:-Unknown}"
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
  denied="$(pod_result egress-denied)"
  open="$(pod_result egress-open)"
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
  result="$(pod_result oidc)"
  if [[ "${result}" == Succeeded ]]; then
    report PASS "OIDC discovery and JWKS reachable from a pod"
  else
    report FAIL "OIDC discovery not reachable from a pod (${result})"
  fi
}

check_pvc() {
  local result phase
  result="$(pod_result pvc)"
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
