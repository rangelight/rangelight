#!/usr/bin/env bash
# Self-tests for scripts/check.sh. Each test builds a throwaway git repo under
# /tmp/rl-check-test.*, copies check.sh into it, adds fixtures and runs one part.
# Usage: scripts/test/check.sh [test_name...]   (no argument runs every test)
set -euo pipefail

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

# --- AC1: size (ADR 0049 §7) -------------------------------------------------

test_size_passes_at_400_lines() {
  lines shared/rust/domain/src/lib.rs 400
  run size
  expect_status 0
}

test_size_fails_over_400_lines() {
  lines shared/rust/domain/src/lib.rs 401
  run size
  expect_status 1
  expect_out "shared/rust/domain/src/lib.rs: 401 lines (limit 400)"
}

test_size_counts_every_source_extension() {
  local ext
  for ext in rs go ts tsx sh sql; do lines "src/big.${ext}" 401; done
  run size
  expect_status 1
  for ext in rs go ts tsx sh sql; do expect_out "src/big.${ext}: 401 lines"; done
}

test_size_skips_tests_generated_and_docs() {
  lines shared/rust/domain/tests/big.rs 900
  lines shared/rust/domain/src/tests.rs 900
  lines services/runner/internal/session/big_test.go 900
  lines services/agent/src/big.test.ts 900
  lines shared/go/gen/big.go 900
  lines shared/rust/proto/src/big.rs 900
  lines shared/rust/store/.sqlx/big.sql 900
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
  lines services/api/src/main.rs 80
  lines services/runner/main.go 80
  lines services/agent/src/main.ts 80
  run size
  expect_status 0
  lines services/api/src/main.rs 81
  lines services/runner/main.go 81
  lines services/agent/src/main.ts 81
  run size
  expect_status 1
  expect_out "services/api/src/main.rs: 81 lines (entrypoint limit 80)"
  expect_out "services/runner/main.go: 81 lines (entrypoint limit 80)"
  expect_out "services/agent/src/main.ts: 81 lines (entrypoint limit 80)"
}

test_size_honours_exception_list() {
  lines services/web/src/big.tsx 450
  put scripts/size-exceptions.txt "# path, one per line
services/web/src/big.tsx"
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
  put scripts/scrub-allow.txt "the Quokka project"
  run scrub RANGELIGHT_SCRUB_PATTERN='zebra|quokka'
  expect_status 0
}

# --- rules (ADR 0049 §2.4, §5.1, §6, §10) --------------------------------------

test_rules_pass_clean_tree() {
  put services/runner/internal/config/config.go 'v := os.Getenv("RANGELIGHT_RUNNER_X")'
  put services/agent/src/config.ts 'const v = process.env["X"];'
  put shared/rust/config/src/lib.rs 'let v = std::env::var("X");'
  put shared/rust/store/src/lane.rs 'sqlx::query_file_as!(Lane, "db/queries/lane/get.sql")'
  put services/web/src/styles/tokens.css ':root { --color-rl: #2f6fed; }'
  put services/runner/main.go "// ${TAG}(RAN-12): split this"
  run rules
  expect_status 0
}

test_rules_env_read_outside_config() {
  put services/runner/internal/session/s.go 'v := os.Getenv("X")'
  put services/agent/src/worker.ts 'const v = process.env.X;'
  put services/api/src/lib.rs 'let v = std::env::var("X");'
  run rules
  expect_status 1
  expect_out "services/runner/internal/session/s.go:1"
  expect_out "services/agent/src/worker.ts:1"
  expect_out "services/api/src/lib.rs:1"
}

test_rules_sql_literal_in_rust() {
  put shared/rust/store/src/a.rs 'sqlx::query!("SELECT 1")'
  put shared/rust/store/src/b.rs 'sqlx::query_as::<_, Lane>(r#"SELECT * FROM lane"#)'
  run rules
  expect_status 1
  expect_out "shared/rust/store/src/a.rs:1"
  expect_out "shared/rust/store/src/b.rs:1"
}

test_rules_todo_needs_ticket() {
  put services/runner/a.go "// ${TAG}: later"
  put services/agent/src/b.ts "// ${TAG}(ran-1): lower case is not a ticket"
  run rules
  expect_status 1
  expect_out "services/runner/a.go:1"
  expect_out "services/agent/src/b.ts:1"
}

test_rules_raw_colour_outside_tokens() {
  put services/web/src/a.tsx 'const c = "#ff0000";'
  put services/web/src/b.tsx '<p className="text-[#fff]" />'
  put services/web/src/c.css 'p { color: rgb(1, 2, 3); }'
  run rules
  expect_status 1
  expect_out "services/web/src/a.tsx:1"
  expect_out "services/web/src/b.tsx:1"
  expect_out "services/web/src/c.css:1"
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

# --- yaml: one YAML style --------------------------------------------------------

# yaml_fixture: the repo's yamllint config, which is itself checked.
yaml_fixture() { cp "${ROOT}/.yamllint.yaml" "${REPO}/"; }

test_yaml_passes_compliant_file() {
  yaml_fixture
  put deployments/dev/a.yaml '---
name: "ci"
replicas: 1
enabled: true
verbs: ["get", "list"]
items:
  - "resources/role.yaml"'
  run yaml
  expect_status 0
  expect_out "ok: yaml"
}

test_yaml_fails_without_document_start() {
  yaml_fixture
  put a.yaml 'name: "ci"'
  run yaml
  expect_status 1
  expect_out 'a.yaml'
  expect_out 'missing document start "---"'
}

test_yaml_fails_on_unquoted_string() {
  yaml_fixture
  put a.yaml '---
name: ci'
  run yaml
  expect_status 1
  expect_out "string value is not quoted with double quotes"
}

test_yaml_fails_on_yml_extension() {
  yaml_fixture
  put .github/workflows/ci.yml '---
name: "ci"'
  run yaml
  expect_status 1
  expect_out ".github/workflows/ci.yml: rename to .yaml"
}

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

# --- hooks (RAN-17): needs a test, runs it, and finds it in a large tree (grep -q + pipefail) ---
test_hooks_needs_and_runs_the_git_mirror_tests() {
  put services/git-mirror/hooks/pre-receive 'exit 1'; run hooks; expect_status 1; expect_out "has no *_test.sh"
  put services/git-mirror/refs_test.sh 'echo ran-refs-test; exit 1'; run hooks; expect_status 1; expect_out "ran-refs-test"
  put services/git-mirror/refs_test.sh 'echo ran-refs-test'
  for i in $(seq 1 500); do put "z/file-${i}.txt" x; done
  run hooks; expect_status 0; expect_out "ok: hooks"
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
