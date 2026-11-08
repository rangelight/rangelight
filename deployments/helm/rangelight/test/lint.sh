#!/usr/bin/env bash
# Lints the chart: vendored dependencies matching Chart.lock, helm lint, kubeconform on what both
# modes render, and our own templates' output in the one YAML style. Run by `scripts/check.sh deploy`, which
# checks that helm, kubeconform and yamllint are the versions pinned in scripts/tools.env.
set -euo pipefail
shopt -s inherit_errexit
export LC_ALL=C

ROOT="$(git rev-parse --show-toplevel)"
CHART="${ROOT}/deployments/helm/rangelight"
# The CRD schemas kubeconform checks custom resources against: this commit holds CloudNativePG 1.30.1's,
# the operator version the chart pins.
CRD_SCHEMAS="https://raw.githubusercontent.com/datreeio/CRDs-catalog/61bcda00d70c1f40e6f4a5e51b5c159aac932860/{{.Group}}/{{.ResourceKind}}_{{.ResourceAPIVersion}}.json"
CNPG_API="postgresql.cnpg.io/v1/Cluster"

die() {
  printf 'chart lint: %s\n' "$1" >&2
  exit 1
}

# render [helm template flags...]: the chart as it installs into rangelight-system, with the values it requires.
render() {
  helm template rangelight "${CHART}" --namespace rangelight-system -f "${CHART}/test/api-values.yaml" \
    --set publicUrl=https://rangelight.example.com --set dex.github.org=rangelight "$@"
}

main() {
  local tool tmp templates=()
  for tool in helm kubeconform yamllint; do command -v "${tool}" >/dev/null || die "missing tool: ${tool}"; done
  # shellcheck source=deployments/dev/versions.env
  source "${ROOT}/deployments/dev/versions.env"

  ! helm dependency list "${CHART}" | awk 'NR > 1 && NF && $NF != "ok" { print; bad = 1 } END { exit !bad }' >&2 ||
    die "charts/ does not match Chart.lock; run: helm dependency build deployments/helm/rangelight"
  helm lint --strict --namespace rangelight-system -f "${CHART}/test/api-values.yaml" \
    --set publicUrl=https://rangelight.example.com --set dex.github.org=rangelight "${CHART}"

  tmp="$(mktemp -d /tmp/rl-chart-lint.XXXXXX)"
  # shellcheck disable=SC2064 # expand now: tmp is local
  trap "rm -rf '${tmp}'" EXIT
  # The git mirror and the runner are off by default (their images are pinned per install): render them on too.
  render --api-versions "${CNPG_API}" -f "${CHART}/test/git-mirror-values.yaml" -f "${CHART}/test/runner-values.yaml" >"${tmp}/bundled.yaml"
  render --set postgres.enabled=false --set postgres.external.secretRef.name=db \
    --set nats.enabled=false --set nats.external.url=nats://nats.example:4222 \
    --set dex.enabled=false --set oidc.issuer=https://idp.example.com >"${tmp}/external.yaml"
  # The schema sets have no schema for a CRD object itself; the vendored CRDs are upstream's, pinned in Chart.lock.
  kubeconform -strict -summary -skip CustomResourceDefinition -kubernetes-version "${KUBERNETES_MIN}.0" -cache "${tmp}" \
    -schema-location default -schema-location "${CRD_SCHEMAS}" "${tmp}/bundled.yaml" "${tmp}/external.yaml"

  # Templates are YAML only once rendered (.yamllint.yaml ignores them): lint what ours render.
  # validate.yaml renders nothing; it only refuses.
  mapfile -t templates < <(cd "${CHART}" && find templates -name '*.yaml' ! -name validate.yaml | sort | sed 's/^/--show-only=/')
  render --api-versions "${CNPG_API}" -f "${CHART}/test/git-mirror-values.yaml" -f "${CHART}/test/runner-values.yaml" "${templates[@]}" | yamllint --strict --config-file "${ROOT}/.yamllint.yaml" - ||
    die "our templates render YAML that breaks the YAML style"
  printf 'chart lint: ok\n'
}

main "$@"
