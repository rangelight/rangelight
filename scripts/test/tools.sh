#!/usr/bin/env bash
# Self-tests for scripts/tools.sh and scripts/tools.env (RAN-42). No network: each test copies tools.sh into
# /tmp/rl-tools-test.* with a one-tool tools.env, and a stub curl that serves a local archive and logs each call.
# Usage: scripts/test/tools.sh [test_name...]   (no argument runs every test)
set -euo pipefail
export LC_ALL=C

ROOT="$(git rev-parse --show-toplevel)"
DIR=""
OUT=""
STATUS=0
FAILED=0

# setup [sha256]: tools.sh, the archive, the curl stub, and a tools.env pinning only gitleaks, to that sha256
# (default: the archive's own).
setup() {
  DIR="$(mktemp -d /tmp/rl-tools-test.XXXXXX)"
  mkdir -p "${DIR}/repo/scripts" "${DIR}/bin" "${DIR}/archive"
  cp "${ROOT}/scripts/tools.sh" "${DIR}/repo/scripts/"
  printf '#!/bin/sh\necho "gitleaks stand-in"\n' >"${DIR}/archive/gitleaks"
  chmod +x "${DIR}/archive/gitleaks"
  tar -czf "${DIR}/gitleaks.tar.gz" -C "${DIR}/archive" gitleaks
  local plat sum="${1:-}"
  plat="$(uname -m | sed -e 's/x86_64/amd64/' -e 's/aarch64/arm64/')"
  [[ -n "${sum}" ]] || sum="$(sha256sum "${DIR}/gitleaks.tar.gz" | awk '{ print $1 }')"
  printf 'TOOLS="gitleaks"\nGITLEAKS=9.9.9\nGITLEAKS_SHA256_linux_%s=%s\n' "${plat}" "${sum}" >"${DIR}/repo/scripts/tools.env"
  # curl -sSfL --retry 3 -o <file> <url>
  cat >"${DIR}/bin/curl" <<EOF
#!/usr/bin/env bash
printf '%s\n' "\${@: -1}" >>"${DIR}/curl.log"
cp "${DIR}/gitleaks.tar.gz" "\${@: -2:1}"
EOF
  chmod +x "${DIR}/bin/curl"
}

tools() {
  STATUS=0
  OUT="$(env PATH="${DIR}/bin:${PATH}" XDG_CACHE_HOME="${DIR}/cache" bash "${DIR}/repo/scripts/tools.sh" "$@" 2>&1)" || STATUS=$?
}

fail() {
  printf '  FAIL: %s\n  --- output ---\n%s\n  --------------\n' "$1" "${OUT}"
  FAILED=1
}

downloads() { if [[ -f "${DIR}/curl.log" ]]; then awk 'END { print NR }' "${DIR}/curl.log"; else printf 0; fi; }

test_installs_once_and_a_second_run_downloads_nothing() {
  setup
  tools
  [[ "${STATUS}" -eq 0 ]] || fail "install failed"
  [[ "$("${DIR}/cache/rangelight/tools/gitleaks-9.9.9/bin/gitleaks")" == "gitleaks stand-in" ]] || fail "gitleaks not installed"
  [[ "$(downloads)" -eq 1 ]] || fail "expected one download, got $(downloads)"
  tools
  [[ "${STATUS}" -eq 0 && "$(downloads)" -eq 1 ]] || fail "the second run downloaded again"
  tools path
  [[ "${OUT}" == "${DIR}/cache/rangelight/tools/gitleaks-9.9.9/bin" ]] || fail "path: ${OUT}"
}

test_a_tampered_download_fails_the_checksum_and_installs_nothing() {
  setup 0000000000000000000000000000000000000000000000000000000000000000
  tools
  [[ "${STATUS}" -ne 0 ]] || fail "a tampered archive was accepted"
  [[ "${OUT}" == *"does not match its sha256 in scripts/tools.env"* ]] || fail "no checksum message"
  [[ ! -e "${DIR}/cache/rangelight/tools/gitleaks-9.9.9" ]] || fail "a tampered tool was installed"
  [[ -z "$(find "${DIR}/cache/rangelight/tools" -mindepth 1 -maxdepth 1)" ]] || fail "a partial install was left behind"
}

test_unknown_command_prints_usage_and_exits_2() {
  setup
  tools nope
  [[ "${STATUS}" -eq 2 && "${OUT}" == *"usage: scripts/tools.sh"* ]] || fail "expected usage and exit 2"
}

# No version or checksum of tools.env is written anywhere else in .github/ or scripts/.
test_no_pin_outside_tools_env() {
  local value hits
  while IFS= read -r value; do
    hits="$(cd "${ROOT}" && git ls-files --cached --others --exclude-standard -- .github scripts |
      grep -vxF scripts/tools.env | xargs -r -d '\n' grep -nHF -e "${value}" -- || true)"
    [[ -z "${hits}" ]] || { OUT="${hits}"; fail "${value} is pinned outside scripts/tools.env"; }
  done < <(grep -oE '^[A-Z0-9_]+=[0-9a-f.]+$' "${ROOT}/scripts/tools.env" | cut -d= -f2 | sort -u)
}

main() {
  local names=("$@") name
  if [[ ${#names[@]} -eq 0 ]]; then
    mapfile -t names < <(declare -F | awk '{print $3}' | grep '^test_')
  fi
  for name in "${names[@]}"; do
    if ! declare -f "${name}" >/dev/null || [[ "${name}" != test_* ]]; then
      printf 'usage: scripts/test/tools.sh [test_name...]\n' >&2
      exit 2
    fi
    printf '%s\n' "${name}"
    "${name}"
    rm -rf "${DIR}"
  done
  if [[ "${FAILED}" -ne 0 ]]; then
    printf 'tools self-tests: FAILED\n'
    exit 1
  fi
  printf 'tools self-tests: ok (%d)\n' "${#names[@]}"
}

main "$@"
