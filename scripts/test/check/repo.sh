# shellcheck shell=bash
# Self-tests of scripts/check/repo.sh: size, scrub, rules. Sourced by scripts/test/check.sh, which has the harness.

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
