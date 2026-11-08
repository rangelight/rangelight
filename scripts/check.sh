#!/usr/bin/env bash
# The whole local gate, and exactly what CI runs (ADR 0049, amendment 2026-11-07).
# Usage: scripts/check.sh <part>    Run without a part to list them.
# The parts live in scripts/check/, one file per group; this file has the helpers. The tools are pinned in
# scripts/tools.env and installed by scripts/tools.sh, whose cache comes first on PATH.
set -euo pipefail
shopt -s inherit_errexit
# One sort order and one message language for everything the checks run.
export LC_ALL=C

ROOT="$(git rev-parse --show-toplevel)"
PARTS=(size rules secrets scrub spec yaml rust go ts deploy hooks fmt gen all)
CHART="deployments/helm/rangelight"
# shellcheck source=scripts/tools.env
source "${ROOT}/scripts/tools.env"
PATH="$(bash "${ROOT}/scripts/tools.sh" path):${PATH}"

# --- helpers -------------------------------------------------------------------

# Every file in the tree that git doesn't ignore: tracked, plus new files not yet added.
repo_files() {
  git -c core.quotePath=false ls-files --cached --others --exclude-standard | sort -u |
    while IFS= read -r f; do [[ -f "${f}" ]] && printf '%s\n' "${f}"; done
}

# Reads all of its input: `grep -q` would exit early and, under pipefail, SIGPIPE reads as "no files".
has_files() { repo_files | grep -E "$1" >/dev/null; }

# The toolchain pinned in go.mod. An exported GOTOOLCHAIN wins over go.mod, so every go call here names this one.
go_toolchain() { awk '$1 == "toolchain" { print $2 }' go.mod; }

# pinned_and_found <tool>: the pinned version, then the version on PATH (empty when it is missing).
pinned_and_found() {
  case "$1" in
    gitleaks) printf '%s\n' "${GITLEAKS}" "$(gitleaks version)" ;;
    buf) printf '%s\n' "${BUF}" "$(buf --version)" ;;
    shellcheck) printf '%s\n' "${SHELLCHECK}" "$(shellcheck --version | awk '$1 == "version:" { print $2 }')" ;;
    helm) printf '%s\n' "${HELM}" "$(helm version --template '{{.Version}}' | sed 's/^v//')" ;;
    kubeconform) printf '%s\n' "${KUBECONFORM}" "$(kubeconform -v | sed 's/^v//')" ;;
    golangci-lint) printf '%s\n' "${GOLANGCI_LINT}" "$(golangci-lint version | awk '{ print $4 }')" ;;
    govulncheck) printf '%s\n' "${GOVULNCHECK}" "$(govulncheck -version | awk -F@v '/^Scanner:/ { print $2 }')" ;;
    cargo-nextest) printf '%s\n' "${CARGO_NEXTEST}" "$(cargo-nextest --version | awk 'NR == 1 { print $2 }')" ;;
    cargo-deny) printf '%s\n' "${CARGO_DENY}" "$(cargo-deny --version | awk '{ print $2 }')" ;;
    cargo-machete) printf '%s\n' "${CARGO_MACHETE}" "$(cargo-machete --version)" ;;
    cargo-sqlx) printf '%s\n' "${SQLX_CLI}" "$(cargo-sqlx sqlx --version | awk '{ print $2 }')" ;;
    nats-server) printf '%s\n' "${NATS_SERVER}" "$(nats-server --version | sed 's/^nats-server: v//')" ;;
    dex) printf '%s\n' "${DEX}" "$(dex version | sed -n 's/^Dex Version: v//p')" ;;
    # The binaries' release adds a build number to PostgreSQL's own version.
    postgres) printf '%s\n' "${POSTGRESQL%.*}" "$(postgres --version | awk '{ print $3 }')" ;;
    yamllint) printf '%s\n' "${YAMLLINT}" "$(yamllint --version | awk '{ print $2 }')" ;;
    check-jsonschema) printf '%s\n' "${CHECK_JSONSCHEMA}" "$(check-jsonschema --version | awk '{ print $3 }')" ;;
    # The language toolchains stay pinned in their own files.
    go) printf '%s\n' "$(go_toolchain)" "$(GOTOOLCHAIN="$(go_toolchain)" command go env GOVERSION)" ;;
    cargo) printf '%s\n' "$(awk -F'"' '$1 ~ /^channel/ { print $2 }' rust-toolchain.toml)" "$(rustc --version | awk '{ print $2 }')" ;;
    node) printf '%s\n' "$(cat .nvmrc)" "$(node --version | sed 's/^v//')" ;;
    pnpm) printf '%s\n' "$(sed -nE 's/.*"packageManager": "pnpm@([^+"]*).*/\1/p' package.json)" "$(pnpm --version)" ;;
    *) printf 'need: %s has no pin\n' "$1" >&2; return 1 ;;
  esac
}

# need <tool...>: each is on PATH, at its pinned version (type -P: a part may share a tool's name, like go).
need() {
  local tool want have
  for tool in "$@"; do
    want="" have=""
    if type -P "${tool}" >/dev/null; then
      { read -r want; read -r have; } < <(pinned_and_found "${tool}" 2>/dev/null || true)
    else
      want="$(pinned_and_found "${tool}" 2>/dev/null | head -1 || true)"
    fi
    [[ -n "${want}" && "${have}" == "${want}" ]] ||
      { printf 'need %s %s, have %s; run scripts/tools.sh\n' "${tool}" "${want:-?}" "${have:-none}" >&2; return 1; }
  done
}

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
