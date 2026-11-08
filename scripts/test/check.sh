#!/usr/bin/env bash
# Self-tests for scripts/check.sh. Each test builds a throwaway git repo under
# /tmp/rl-check-test.*, copies check.sh, its parts and the tool pins into it, adds fixtures and runs one part.
# The tests live in scripts/test/check/, one file per part file.
# Usage: scripts/test/check.sh [test_name...]   (no argument runs every test)
set -euo pipefail
export LC_ALL=C

ROOT="$(git rev-parse --show-toplevel)"
CHECK="${ROOT}/scripts/check.sh"
# Built from parts so this file never trips the rules part itself.
TAG="TO""DO"
REPO=""
OUT=""
STATUS=0
FAILED=0

new_repo() {
  REPO="$(mktemp -d /tmp/rl-check-test.XXXXXX)"
  git -C "${REPO}" init -q
  mkdir -p "${REPO}/scripts"
  cp "${CHECK}" "${REPO}/scripts/check.sh"
  cp -R "${ROOT}/scripts/check" "${REPO}/scripts/check"
  cp "${ROOT}/scripts/tools.env" "${ROOT}/scripts/tools.sh" "${REPO}/scripts/"
  : >"${REPO}/scripts/size-exceptions.txt"
  : >"${REPO}/scripts/scrub-allow.txt"
}

# put <path> <content>: write a fixture file inside the test repo.
put() {
  mkdir -p "$(dirname "${REPO}/$1")"
  printf '%s\n' "$2" >"${REPO}/$1"
}

# lines <path> <n>: write a fixture of exactly n lines.
lines() {
  mkdir -p "$(dirname "${REPO}/$1")"
  seq 1 "$2" | sed 's/^/x/' >"${REPO}/$1"
}

# run <part> [env assignments...]: run check.sh in the test repo with a clean env.
run() {
  local part="$1"
  shift
  STATUS=0
  OUT="$(cd "${REPO}" && env -u RANGELIGHT_SCRUB_PATTERN -u RANGELIGHT_SCRUB_REQUIRED \
    "$@" bash scripts/check.sh "${part}" 2>&1)" || STATUS=$?
}

fail() {
  printf '  FAIL: %s\n  --- output ---\n%s\n  --------------\n' "$1" "${OUT}"
  FAILED=1
}

expect_status() { [[ "${STATUS}" -eq "$1" ]] || fail "expected exit $1, got ${STATUS}"; }
expect_out() { [[ "${OUT}" == *"$1"* ]] || fail "expected output to contain: $1"; }
expect_no_out() { [[ "${OUT}" != *"$1"* ]] || fail "expected output not to contain: $1"; }

# shellcheck source=scripts/test/check/repo.sh
source "${ROOT}/scripts/test/check/repo.sh"
# shellcheck source=scripts/test/check/spec.sh
source "${ROOT}/scripts/test/check/spec.sh"
# shellcheck source=scripts/test/check/code.sh
source "${ROOT}/scripts/test/check/code.sh"

# --- dispatcher and empty parts ------------------------------------------------

test_unknown_part_prints_usage_and_exits_2() {
  run nope
  expect_status 2
  expect_out "usage: scripts/check.sh"
  run main
  expect_status 2
}

test_parts_without_sources_skip() {
  local part
  for part in rust go ts hooks; do
    run "${part}"
    expect_status 0
    expect_out "skipped: ${part} (no sources yet)"
  done
}

# --- need: every tool at its pinned version (RAN-42) ----------------------------

# stub_tool <name> <output>: a fake tool that prints output, in REPO/stub; an empty tool cache, so it wins.
stub_tool() {
  mkdir -p "${REPO}/stub" "${REPO}/cache"
  printf '#!/usr/bin/env bash\necho "%s"\n' "$2" >"${REPO}/stub/$1"
  chmod +x "${REPO}/stub/$1"
}

# pinned <KEY>: its value in scripts/tools.env.
pinned() { bash -c 'source "$1"; printf "%s" "${!2}"' _ "${ROOT}/scripts/tools.env" "$1"; }

test_need_fails_on_a_version_mismatch() {
  stub_tool yamllint "yamllint 1.0.0"
  run yaml "PATH=${REPO}/stub:${PATH}" "XDG_CACHE_HOME=${REPO}/cache"
  expect_status 1
  expect_out "need yamllint $(pinned YAMLLINT), have 1.0.0; run scripts/tools.sh"
  expect_out "FAIL: yaml (tools)"
}

test_need_fails_on_a_missing_tool() {
  local tool
  stub_tool unused ""
  # Only what check.sh runs before need, so no yamllint from anywhere on the machine.
  for tool in bash env git dirname head; do ln -s "$(type -P "${tool}")" "${REPO}/stub/${tool}"; done
  run yaml "PATH=${REPO}/stub" "XDG_CACHE_HOME=${REPO}/cache"
  expect_status 1
  expect_out "need yamllint $(pinned YAMLLINT), have none; run scripts/tools.sh"
}

# --- LC_ALL=C: one sort order in every test entry point (RAN-42) ----------------

test_every_test_entry_point_sets_lc_all() {
  local f
  while IFS= read -r f; do
    grep -qxE 'export LC_ALL=C( .*)?|source "\$\(dirname "\$\{BASH_SOURCE\[0\]\}"\)/lib\.sh"' "${ROOT}/${f}" ||
      { OUT="${f}"; fail "no export LC_ALL=C (nor lib.sh, which has it)"; }
  done < <(cd "${ROOT}" && git ls-files --cached --others --exclude-standard -- scripts/check.sh scripts/tools.sh \
    ':(glob)scripts/test/*.sh' ':(glob)deployments/helm/rangelight/test/*.sh' ':(glob)services/**/*_test.sh')
  grep -qxF 'export LC_ALL=C' "${ROOT}/deployments/helm/rangelight/test/lib.sh" || fail "lib.sh has no export LC_ALL=C"
}

main() {
  local names=("$@") name
  if [[ ${#names[@]} -eq 0 ]]; then
    mapfile -t names < <(declare -F | awk '{print $3}' | grep '^test_')
  fi
  for name in "${names[@]}"; do
    if ! declare -f "${name}" >/dev/null || [[ "${name}" != test_* ]]; then
      printf 'usage: scripts/test/check.sh [test_name...]\n' >&2
      exit 2
    fi
    printf '%s\n' "${name}"
    new_repo
    "${name}"
    rm -rf "${REPO}"
  done
  if [[ "${FAILED}" -ne 0 ]]; then
    printf 'self-tests: FAILED\n'
    exit 1
  fi
  printf 'self-tests: ok (%d)\n' "${#names[@]}"
}

main "$@"
