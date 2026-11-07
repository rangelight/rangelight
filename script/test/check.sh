#!/usr/bin/env bash
# Self-tests for script/check.sh. Each test builds a throwaway git repo under
# /tmp/rl-check-test.*, copies check.sh into it, adds fixtures and runs one part.
# Usage: script/test/check.sh [test_name...]   (no argument runs every test)
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel)"
CHECK="${ROOT}/script/check.sh"
# Built from parts so this file never trips the rules part itself.
TAG="TO""DO"
REPO=""
OUT=""
STATUS=0
FAILED=0

new_repo() {
  REPO="$(mktemp -d /tmp/rl-check-test.XXXXXX)"
  git -C "${REPO}" init -q
  mkdir -p "${REPO}/script"
  cp "${CHECK}" "${REPO}/script/check.sh"
  : >"${REPO}/script/size-exceptions.txt"
  : >"${REPO}/script/scrub-allow.txt"
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
    "$@" bash script/check.sh "${part}" 2>&1)" || STATUS=$?
}

fail() {
  printf '  FAIL: %s\n  --- output ---\n%s\n  --------------\n' "$1" "${OUT}"
  FAILED=1
}

expect_status() { [[ "${STATUS}" -eq "$1" ]] || fail "expected exit $1, got ${STATUS}"; }
expect_out() { [[ "${OUT}" == *"$1"* ]] || fail "expected output to contain: $1"; }
expect_no_out() { [[ "${OUT}" != *"$1"* ]] || fail "expected output not to contain: $1"; }

# --- AC1: size (ADR 0049 §7) -------------------------------------------------

test_size_passes_at_400_lines() {
  lines control-plane/crates/rl-domain/src/lib.rs 400
  run size
  expect_status 0
}

test_size_fails_over_400_lines() {
  lines control-plane/crates/rl-domain/src/lib.rs 401
  run size
  expect_status 1
  expect_out "control-plane/crates/rl-domain/src/lib.rs: 401 lines (limit 400)"
}

test_size_counts_every_source_extension() {
  local ext
  for ext in rs go ts tsx sh sql; do lines "src/big.${ext}" 401; done
  run size
  expect_status 1
  for ext in rs go ts tsx sh sql; do expect_out "src/big.${ext}: 401 lines"; done
}

test_size_skips_tests_generated_and_docs() {
  lines control-plane/tests/big.rs 900
  lines control-plane/crates/rl-domain/src/tests.rs 900
  lines runner/internal/session/big_test.go 900
  lines agent/src/big.test.ts 900
  lines api/gen/go/big.go 900
  lines control-plane/crates/rl-proto/src/big.rs 900
  lines control-plane/.sqlx/big.sql 900
  lines docs/big.md 900
  lines big.json 900
  lines big.yaml 900
  run size
  expect_status 0
}

test_size_skips_ignored_files() {
  put .gitignore "target/"
  lines target/debug/big.rs 900
  run size
  expect_status 0
}

test_size_entrypoint_limit_is_80() {
  lines control-plane/services/api/src/main.rs 80
  lines runner/cmd/runner/main.go 80
  lines agent/src/main.ts 80
  run size
  expect_status 0
  lines control-plane/services/api/src/main.rs 81
  lines runner/cmd/runner/main.go 81
  lines agent/src/main.ts 81
  run size
  expect_status 1
  expect_out "control-plane/services/api/src/main.rs: 81 lines (entrypoint limit 80)"
  expect_out "runner/cmd/runner/main.go: 81 lines (entrypoint limit 80)"
  expect_out "agent/src/main.ts: 81 lines (entrypoint limit 80)"
}

test_size_honours_exception_list() {
  lines web/src/big.tsx 450
  put script/size-exceptions.txt "# path, one per line
web/src/big.tsx"
  run size
  expect_status 0
}

# --- AC2: scrub ----------------------------------------------------------------

test_scrub_skips_without_pattern() {
  put README.md "hello"
  run scrub
  expect_status 0
  expect_out "skipped: scrub (no pattern)"
}

test_scrub_fails_closed_when_required() {
  put README.md "hello"
  run scrub RANGELIGHT_SCRUB_REQUIRED=true
  expect_status 1
  expect_out "RANGELIGHT_SCRUB_PATTERN is not set"
}

test_scrub_passes_clean_tree() {
  put README.md "hello"
  run scrub RANGELIGHT_SCRUB_PATTERN='zebra|quokka'
  expect_status 0
  expect_out "ok: scrub"
}

test_scrub_reports_location_not_content() {
  put README.md "hello"
  put docs/notes.md "line one
the Quokka project"
  run scrub RANGELIGHT_SCRUB_PATTERN='zebra|quokka'
  expect_status 1
  expect_out "docs/notes.md:2"
  expect_no_out "Quokka"
}

test_scrub_rejects_invalid_pattern() {
  put README.md "hello"
  run scrub RANGELIGHT_SCRUB_PATTERN='('
  expect_status 1
  expect_out "FAIL: scrub (invalid pattern)"
}

test_scrub_honours_allow_list() {
  put docs/notes.md "the Quokka project"
  put script/scrub-allow.txt "the Quokka project"
  run scrub RANGELIGHT_SCRUB_PATTERN='zebra|quokka'
  expect_status 0
}

# --- rules (ADR 0049 §2.4, §5.1, §6, §10) --------------------------------------

test_rules_pass_clean_tree() {
  put services/runner/internal/config/config.go 'v := os.Getenv("RANGELIGHT_RUNNER_X")'
  put agent/src/config.ts 'const v = process.env["X"];'
  put control-plane/crates/rl-config/src/lib.rs 'let v = std::env::var("X");'
  put control-plane/crates/rl-store/src/lane.rs 'sqlx::query_file_as!(Lane, "queries/lane/get.sql")'
  put web/src/styles/tokens.css ':root { --color-rl: #2f6fed; }'
  put runner/main.go "// ${TAG}(RAN-12): split this"
  run rules
  expect_status 0
}

test_rules_env_read_outside_config() {
  put runner/internal/session/s.go 'v := os.Getenv("X")'
  put agent/src/worker.ts 'const v = process.env.X;'
  put control-plane/services/api/src/lib.rs 'let v = std::env::var("X");'
  run rules
  expect_status 1
  expect_out "runner/internal/session/s.go:1"
  expect_out "agent/src/worker.ts:1"
  expect_out "control-plane/services/api/src/lib.rs:1"
}

test_rules_sql_literal_in_rust() {
  put control-plane/crates/rl-store/src/a.rs 'sqlx::query!("SELECT 1")'
  put control-plane/crates/rl-store/src/b.rs 'sqlx::query_as::<_, Lane>(r#"SELECT * FROM lane"#)'
  run rules
  expect_status 1
  expect_out "control-plane/crates/rl-store/src/a.rs:1"
  expect_out "control-plane/crates/rl-store/src/b.rs:1"
}

test_rules_todo_needs_ticket() {
  put runner/a.go "// ${TAG}: later"
  put agent/src/b.ts "// ${TAG}(ran-1): lower case is not a ticket"
  run rules
  expect_status 1
  expect_out "runner/a.go:1"
  expect_out "agent/src/b.ts:1"
}

test_rules_raw_colour_outside_tokens() {
  put web/src/a.tsx 'const c = "#ff0000";'
  put web/src/b.tsx '<p className="text-[#fff]" />'
  put web/src/c.css 'p { color: rgb(1, 2, 3); }'
  run rules
  expect_status 1
  expect_out "web/src/a.tsx:1"
  expect_out "web/src/b.tsx:1"
  expect_out "web/src/c.css:1"
}

# --- spec (RAN-15) -------------------------------------------------------------

# spec_fixture: the repo's real contracts, generated code and embedded schema copies.
spec_fixture() {
  mkdir -p "${REPO}/services/runner/internal/session" "${REPO}/shared/go"
  cp -R "${ROOT}/api" "${REPO}/api"
  cp "${ROOT}/go.mod" "${ROOT}/go.sum" "${REPO}/"
  cp -R "${ROOT}/shared/go/gen" "${REPO}/shared/go/"
  cp -R "${ROOT}/services/runner/internal/session/schema" "${REPO}/services/runner/internal/session/"
}

test_spec_fails_without_contract_files() {
  run spec
  expect_status 1
  expect_out "spec: missing api/buf.yaml"
  expect_out "spec: missing api/proto/rangelight/runner/v1alpha1/runner.proto"
  expect_out "spec: missing api/jsonschema/agent-event.v1alpha1.schema.json"
  expect_out "FAIL: spec (missing files)"
}

test_spec_passes_with_contracts() {
  spec_fixture
  run spec
  expect_status 0
  expect_out "ok: spec"
}

test_spec_fails_when_embedded_copy_differs() {
  spec_fixture
  printf ' \n' >>"${REPO}/services/runner/internal/session/schema/rangelight.v1alpha1.schema.json"
  run spec
  expect_status 1
  expect_out "services/runner/internal/session/schema/rangelight.v1alpha1.schema.json differs from api/jsonschema/"
}

test_spec_fails_when_invalid_example_validates() {
  spec_fixture
  cp "${REPO}/api/examples/receipt.example.json" "${REPO}/api/examples/invalid/receipt.not-really.json"
  run spec
  expect_status 1
  expect_out "api/examples/invalid/receipt.not-really.json passes"
}

test_spec_fails_when_example_does_not_validate() {
  spec_fixture
  cp "${REPO}/api/examples/invalid/receipt.missing-provenance.json" "${REPO}/api/examples/receipt.broken.json"
  run spec
  expect_status 1
  expect_out "'provenance' is a required property"
  expect_out "FAIL: spec (schemas)"
}

test_spec_fails_on_example_without_schema() {
  spec_fixture
  put api/examples/session.example.json '{}'
  run spec
  expect_status 1
  expect_out "api/examples/session.example.json: no schema for this name"
}

test_spec_fails_on_stale_generated_code() {
  spec_fixture
  printf '// edited by hand\n' >>"${REPO}/shared/go/gen/rangelight/runner/v1alpha1/runner.pb.go"
  run spec
  expect_status 1
  expect_out "shared/go/gen is stale"
}

test_spec_fails_on_breaking_change_against_base() {
  spec_fixture
  git -C "${REPO}" add -A
  git -C "${REPO}" -c user.name=t -c user.email=t@example.com commit -qm base
  sed -i '/string node_name = 7;/d' "${REPO}/api/proto/rangelight/runner/v1alpha1/runner.proto"
  run spec RANGELIGHT_SPEC_BASE=HEAD
  expect_status 1
  expect_out 'Previously present field "7" with name "node_name"'
  expect_out "FAIL: spec (breaking change against HEAD)"
}

# --- dispatcher and empty parts ------------------------------------------------

test_unknown_part_prints_usage_and_exits_2() {
  run nope
  expect_status 2
  expect_out "usage: script/check.sh"
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

main() {
  local names=("$@") name
  if [[ ${#names[@]} -eq 0 ]]; then
    mapfile -t names < <(declare -F | awk '{print $3}' | grep '^test_')
  fi
  for name in "${names[@]}"; do
    if ! declare -f "${name}" >/dev/null || [[ "${name}" != test_* ]]; then
      printf 'usage: script/test/check.sh [test_name...]\n' >&2
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
