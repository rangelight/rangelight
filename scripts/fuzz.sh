#!/usr/bin/env bash
# Fuzzing (ADR 0049 §8, amendment 2026-11-06): every parser of untrusted input has a fuzz target. Run nightly by
# .github/workflows/fuzz.yaml, never by scripts/check.sh (which runs each target's seed corpus as a test).
# Usage: scripts/fuzz.sh <command>
#   go    each Go fuzz target (func FuzzX in a *_test.go) for RANGELIGHT_FUZZ_TIME (default 2m)
set -euo pipefail
shopt -s inherit_errexit

ROOT="$(git rev-parse --show-toplevel)"
COMMANDS=(go)
FUZZ_TIME="${RANGELIGHT_FUZZ_TIME:-2m}"

go() {
  local file target found=0 toolchain
  toolchain="$(awk '$1 == "toolchain" { print $2 }' go.mod)"
  while IFS=: read -r file target; do
    found=1
    printf 'fuzz: %s in ./%s\n' "${target}" "${file%/*}"
    GOTOOLCHAIN="${toolchain}" command go test -run '^$' -fuzz "^${target}\$" -fuzztime "${FUZZ_TIME}" "./${file%/*}" </dev/null
  done < <({ git grep --untracked -o -E '^func Fuzz[A-Za-z0-9_]+' -- '*_test.go' || true; } | sed -E 's/:func /:/')
  [[ "${found}" -eq 1 ]] || printf 'skipped: fuzz go (no targets yet)\n'
}

# --- dispatcher -----------------------------------------------------------------

usage() {
  printf 'usage: scripts/fuzz.sh <command>\ncommands: %s\n' "${COMMANDS[*]}" >&2
}

main() {
  local command="${1:-}"
  if [[ " ${COMMANDS[*]} " != *" ${command} "* ]] || ! declare -f "${command}" >/dev/null; then
    usage
    exit 2
  fi
  shift
  cd "${ROOT}"
  "${command}" "$@"
}

main "$@"
