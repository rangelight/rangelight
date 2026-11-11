#!/usr/bin/env bash
# The chart tests' helpers, sourced by template.sh, git-mirror.sh, runner.sh, dex.sh, api.sh, relay.sh, web.sh and
# ingress.sh.
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

# network_peers: the rendered NetworkPolicy's rules, one line each: direction, port(s), then each peer as
# ns:<labels> pod:<labels> (pod:{} for every pod), ip:<cidr>-<except>..., or "any" for a rule without peers.
network_peers() {
  awk '
    function flush(   line, i) {
      if (dir == "") return
      line = dir " " ports
      if (n == 0) line = line " any"
      for (i = 1; i <= n; i++) line = line " " (ns[i] != "" ? "ns:" ns[i] " " : "") (pod[i] != "" ? "pod:" pod[i] : "") (ip[i] != "" ? "ip:" ip[i] : "")
      sub(/ +$/, "", line)
      print line
      dir = ""
    }
    function label(   kv) { kv = $1 $2; sub(/:/, "=", kv); gsub(/"/, "", kv); return kv }
    /^  (ingress|egress):$/ { flush(); side = $1; sub(/:/, "", side); next }
    /^    - (from|to|ports):$/ { flush(); dir = side; ports = ""; n = 0; next }
    /^        - (podSelector|namespaceSelector|ipBlock):/ { n++; ns[n] = pod[n] = ip[n] = "" }
    /^        - |^          (podSelector|namespaceSelector):/ {
      sel = $0; sub(/^ *-? */, "", sel); sub(/:.*/, "", sel)
      if ($0 ~ /\{\}$/) pod[n] = "{}"
      next
    }
    /^              [^ -][^ ]*: "/ { if (sel == "podSelector") pod[n] = pod[n] (pod[n] == "" ? "" : ",") label(); else ns[n] = ns[n] (ns[n] == "" ? "" : ",") label(); next }
    /^            cidr: / { c = $2; gsub(/"/, "", c); ip[n] = c; next }
    /^              - "/ { c = $2; gsub(/"/, "", c); ip[n] = ip[n] "-" c; next }
    /^          port: / { if (ports != $2 && ports !~ ("(^|,)" $2 "$")) ports = ports (ports == "" ? "" : ",") $2; next }
    /^---/ { flush() }
    END { flush() }
  ' <<<"${OUT}"
}

# expect_restricted [token]: the rendered pod passes Pod Security `restricted`, has a read-only root, resources
# set and, unless `token` (upstream charts' pods keep theirs), no ServiceAccount token. Our templates quote
# strings; upstream charts' may not.
expect_restricted() {
  local want
  for want in "runAsNonRoot: true" "allowPrivilegeEscalation: false" "readOnlyRootFilesystem: true" '- "?ALL"?' \
    'type: "?RuntimeDefault"?' "limits:" "requests:"; do
    grep -qE -e "^ +${want}$" <<<"${OUT}" || fail "pod without '${want}'"
  done
  [[ "${1:-}" == token ]] || grep -qE -e '^ +automountServiceAccountToken: false$' <<<"${OUT}" ||
    fail "pod with a ServiceAccount token"
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
