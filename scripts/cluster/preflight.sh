# The `preflight` command of scripts/cluster.sh, and its probes: what the cluster must offer before `up`.
# Sourced by scripts/cluster.sh (its helpers, k and report, and its globals), never run.
# shellcheck shell=bash

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
