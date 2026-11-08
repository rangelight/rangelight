#!/usr/bin/env bash
# The chart tests' helpers, sourced by template.sh, git-mirror.sh, runner.sh and dex.sh.
# main runs the sourcing file's test_* functions (or the named ones) and fails if any failed.
# One sort order everywhere: the expected lists are in C order.
export LC_ALL=C

CHART="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck disable=SC2034 # used by the files that source this one
CNPG_API="postgresql.cnpg.io/v1/Cluster"
# Your own OIDC provider instead of Dex (RAN-37).
# shellcheck disable=SC2034 # used by the files that source this one
EXTERNAL_OIDC=(--set dex.enabled=false --set oidc.issuer=https://idp.example.com/realms/eng
  --set oidc.clientSecretRef.name=idp-client)
OUT=""
STATUS=0
FAILED=0

# render [helm template flags...]: the chart in rangelight-system with the values it requires: sign-in
# (publicUrl, dex.github.org) and the api's sessions (test/api-values.yaml); later flags win. Into OUT and STATUS.
render() {
  STATUS=0
  OUT="$(helm template rangelight "${CHART}" --namespace rangelight-system -f "${CHART}/test/api-values.yaml" \
    --set publicUrl=https://rangelight.example.com --set dex.github.org=rangelight "$@" 2>&1)" || STATUS=$?
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

main() {
  local tests=() t
  if [[ $# -gt 0 ]]; then tests=("$@"); else mapfile -t tests < <(declare -F | awk '$3 ~ /^test_/ { print $3 }'); fi
  for t in "${tests[@]}"; do
    printf '%s\n' "${t}"
    "${t}"
  done
  [[ "${FAILED}" -eq 0 ]] || { printf 'chart tests (%s): FAILED\n' "${0##*/}" >&2; exit 1; }
  printf 'chart tests (%s): ok\n' "${0##*/}"
}
