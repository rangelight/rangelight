#!/usr/bin/env bash
# What the chart renders for the runner (RAN-19), and the values it refuses. Run by `scripts/check.sh deploy`.
# Usage: deployments/helm/rangelight/test/runner.sh [test_name...]   (no argument runs every test)
set -euo pipefail
# shellcheck source=deployments/helm/rangelight/test/lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

# --- the runner (RAN-19) ------------------------------------------------------------

RUNNER=(-f "${CHART}/test/runner-values.yaml")
RUNNER_KINDS="ClusterRole/rangelight-runner ClusterRoleBinding/rangelight-runner CustomResourceDefinition/sessions.rangelight.dev Deployment/runner NetworkPolicy/runner Role/runner RoleBinding/runner Service/runner ServiceAccount/runner ValidatingAdmissionPolicy/rangelight-runner ValidatingAdmissionPolicyBinding/rangelight-runner"

runner_objects() {
  local t args=()
  for t in crd rbac admission deployment service networkpolicy; do args+=(--show-only "templates/runner/${t}.yaml"); done
  render "${RUNNER[@]}" "$@" "${args[@]}"
}

# RAN-35: NATS admits the runner's pod from its own namespace (the runner protocol), and nothing else.
test_nats_ingress_admits_the_runner() {
  render "${RUNNER[@]}" --show-only templates/nats/networkpolicy.yaml
  expect_ok
  expect_line '              kubernetes.io/metadata.name: "rangelight-runner"'
  expect_line '              app.kubernetes.io/name: "runner"'
  expect_no_match 'ipBlock|port: 8222'
}

test_runner_is_off_by_default() {
  render --api-versions "${CNPG_API}"
  expect_ok
  expect_no_match 'name: "runner"|rangelight-runner|sessions\.rangelight\.dev'
}

test_runner_renders_its_objects_in_its_own_namespace() {
  runner_objects
  expect_ok
  [[ "$(kinds | paste -sd' ')" == "${RUNNER_KINDS}" ]] || fail "the runner renders: $(kinds | paste -sd' ')"
  [[ "$(grep -c '^  namespace: "rangelight-runner"$' <<<"${OUT}")" -eq 6 ]] || fail "a namespaced object outside rangelight-runner"
  runner_objects --set runner.namespace=rl-runner-test
  [[ "$(grep -c '^  namespace: "rl-runner-test"$' <<<"${OUT}")" -eq 6 ]] || fail "runner.namespace is not used everywhere"
}

test_runner_pod_is_restricted_with_one_replica() {
  local want
  render "${RUNNER[@]}" --show-only templates/runner/deployment.yaml
  expect_ok
  for want in "runAsNonRoot: true" "allowPrivilegeEscalation: false" "readOnlyRootFilesystem: true" '- "ALL"' \
    'type: "RuntimeDefault"' "limits:" "requests:"; do
    grep -qE -e "^ +${want}$" <<<"${OUT}" || fail "pod without '${want}'"
  done
  expect_line '  replicas: 1'
  expect_line "          image: \"ghcr.io/rangelight/runner:test@sha256:$(printf '2%.0s' {1..64})\""
  expect_line "              value: \"ghcr.io/rangelight/agent:test@sha256:$(printf '3%.0s' {1..64})\""
  expect_line '              value: "http://git-mirror.rangelight-system.svc.cluster.local:8080"'
  expect_line '              value: "4"'
  expect_line '            sizeLimit: "2Gi"'
  # The runner's own identity at the mirror (RAN-19 review R1).
  expect_line '                  audience: "rangelight-git-mirror"'
  expect_line '              mountPath: "/var/run/secrets/rangelight/git-mirror"'
}

# The admission policy binds the runner's ServiceAccount, fails closed, and denies (review B1); envtest
# proves what it allows (services/runner/internal/session/admission_test.go).
test_runner_admission_policy_confines_the_runner() {
  render "${RUNNER[@]}" --show-only templates/runner/admission.yaml --set runner.namespace=rl-runner-test
  expect_ok
  expect_line '  failurePolicy: "Fail"'
  expect_line '  validationActions: ["Deny"]'
  expect_line "      expression: \"request.userInfo.username == 'system:serviceaccount:rl-runner-test:runner'\""
  grep -qF -e "request.namespace == 'rl-runner-test'" <<<"${OUT}" || fail "Sessions are not confined to runner.namespace"
}

# Least privilege: exactly these rules, and nothing on Secrets, exec, ServiceAccounts or RBAC.
test_runner_rbac_is_least_privilege() {
  local rules
  render "${RUNNER[@]}" --show-only templates/runner/rbac.yaml
  expect_ok
  rules="$(awk '/^kind:/ { k = $2 } /^  - apiGroups:/ { g = $3 } /^    resources:/ { r = $2 } /^    verbs:/ { $1 = ""; print k, g, r, $0 }' <<<"${OUT}" | tr -d '",[]')"
  [[ "${rules}" == "ClusterRole  namespaces  get list watch create delete
ClusterRole  pods  get list watch create
ClusterRole  persistentvolumeclaims  get list watch create
ClusterRole  configmaps  get list watch create update
ClusterRole networking.k8s.io networkpolicies  get list watch create update
ClusterRole cilium.io ciliumnetworkpolicies  get list watch create update
ClusterRole authentication.k8s.io tokenreviews  create
Role rangelight.dev sessions  get list watch create patch update delete
Role rangelight.dev sessions/status  patch" ]] || fail "RBAC rules changed:
${rules}"
  expect_no_match 'secrets|pods/exec|serviceaccounts|"\*"|escalate|bind|impersonate'
}

test_runner_is_reachable_only_from_the_git_mirror() {
  render "${RUNNER[@]}" --show-only templates/runner/networkpolicy.yaml
  expect_ok
  expect_line '              kubernetes.io/metadata.name: "rangelight-system"'
  expect_line '              app.kubernetes.io/name: "git-mirror"'
  expect_line '          port: 8080'
  expect_no_match 'ipBlock'
}

test_runner_gets_the_cluster_dns_from_values() {
  render "${RUNNER[@]}" --show-only templates/runner/deployment.yaml
  expect_line '              value: "k8s-app=kube-dns"'
  # One cluster DNS setting for the whole chart (RAN-37 security review round 2): clusterDns.
  render "${RUNNER[@]}" --show-only templates/runner/deployment.yaml --set clusterDns.podLabel=k8s-app=coredns --set clusterDns.namespace=dns
  expect_line '              value: "k8s-app=coredns"'
  expect_line '              value: "dns"'
  render "${RUNNER[@]}" --set "clusterDns.podLabel=k8s-app"
  expect_refused "/clusterDns/podLabel"
  render "${RUNNER[@]}" --set "runner.dns.podLabel=k8s-app=coredns"
  expect_refused "additional properties 'dns' not allowed"
}

test_refuses_the_runner_in_rangelight_system() {
  render "${RUNNER[@]}" --set runner.namespace=rangelight-system
  expect_refused 'runner.namespace must not be "rangelight-system"'
}

test_refuses_a_runner_without_pinned_images() {
  render --set runner.enabled=true
  expect_refused "/runner/image"
  render "${RUNNER[@]}" --set runner.agentImage=
  expect_refused "/runner/agentImage"
  render "${RUNNER[@]}" --set runner.image=ghcr.io/rangelight/runner:1
  expect_refused "/runner/image"
  render "${RUNNER[@]}" --set runner.workers=0
  expect_refused "/runner/workers"
}

# A template, not crds/: Helm applies crds/ on first install only, so an upgrade would never deliver the CRD.
test_the_session_crd_installs_and_upgrades_with_the_runner_and_outlives_uninstall() {
  [[ ! -e "${CHART}/crds" ]] || fail "crds/ exists: Helm never upgrades what is in it"
  render "${RUNNER[@]}" --show-only templates/runner/crd.yaml
  expect_ok
  expect_line '  name: "sessions.rangelight.dev"'
  expect_line '  scope: "Namespaced"'
  expect_line '    helm.sh/resource-policy: "keep"'
}

main "$@"
