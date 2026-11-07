#!/usr/bin/env bash
# The whole local gate, and exactly what CI runs (ADR 0049, amendment 2026-11-07).
# Usage: scripts/check.sh <part>    Run without a part to list them.
# The parts live in scripts/check/, one file per group; this file has the helpers and pins.
set -euo pipefail
shopt -s inherit_errexit

ROOT="$(git rev-parse --show-toplevel)"
PARTS=(size rules secrets scrub spec yaml rust go ts deploy hooks fmt gen all)
GOLANGCI_LINT="github.com/golangci/golangci-lint/v2/cmd/golangci-lint@v2.15.0"
GOVULNCHECK="golang.org/x/vuln/cmd/govulncheck@v1.9.0"
BUF_VERSION="1.75.0"
CHECK_JSONSCHEMA_VERSION="0.38.2"
YAMLLINT_VERSION="1.38.0"
CHART="deployments/helm/rangelight"

# --- helpers -------------------------------------------------------------------

# Every file in the tree that git doesn't ignore: tracked, plus new files not yet added.
repo_files() {
  git -c core.quotePath=false ls-files --cached --others --exclude-standard | sort -u |
    while IFS= read -r f; do [[ -f "${f}" ]] && printf '%s\n' "${f}"; done
}

# Reads all of its input: `grep -q` would exit early and, under pipefail, SIGPIPE reads as "no files".
has_files() { repo_files | grep -E "$1" >/dev/null; }

need() {
  local tool
  for tool in "$@"; do
    command -v "${tool}" >/dev/null || { printf 'missing tool: %s\n' "${tool}" >&2; return 1; }
  done
}

# The toolchain pinned in go.mod. `go run tool@version` builds the tool with the tool's own go line
# and an exported GOTOOLCHAIN wins over go.mod, so every go call here names this one.
go_toolchain() { awk '$1 == "toolchain" { print $2 }' go.mod; }

ok() { printf 'ok: %s\n' "$1"; }
skip() { printf 'skipped: %s (no sources yet)\n' "$1"; }
failed() { printf 'FAIL: %s\n' "$1" >&2; return 1; }

# listed <path> <file>: is the path a line of the file (ignoring # comments)?
listed() { [[ -f "$2" ]] && grep -v '^#' "$2" | grep -qxF -e "$1"; }

# located <rule>: turn grep's file:line:content into file:line: rule.
located() { awk -F: -v rule="$1" '{ print $1 ":" $2 ": " rule }'; }

# scan <rule> <ere> <files...>: print file:line for each match, without the content.
scan() {
  local rule="$1" re="$2"
  shift 2
  [[ $# -gt 0 ]] || return 0
  { grep -nHE -e "${re}" -- "$@" || true; } | located "${rule}"
}

# --- parts ---------------------------------------------------------------------

# shellcheck source=scripts/check/repo.sh
source "${ROOT}/scripts/check/repo.sh"
# shellcheck source=scripts/check/spec.sh
source "${ROOT}/scripts/check/spec.sh"
# shellcheck source=scripts/check/code.sh
source "${ROOT}/scripts/check/code.sh"

all() {
  local part bad=()
  for part in size rules secrets scrub spec yaml rust go ts deploy hooks; do
    bash "${ROOT}/scripts/check.sh" "${part}" || bad+=("${part}")
  done
  [[ ${#bad[@]} -eq 0 ]] || failed "all (${bad[*]})"
  ok all
}

# --- dispatcher -----------------------------------------------------------------

usage() {
  printf 'usage: scripts/check.sh <part>\nparts: %s\n' "${PARTS[*]}" >&2
}

main() {
  local part="${1:-}"
  if [[ " ${PARTS[*]} " != *" ${part} "* ]] || ! declare -f "${part}" >/dev/null; then
    usage
    exit 2
  fi
  shift
  cd "${ROOT}"
  "${part}" "$@"
}

main "$@"
