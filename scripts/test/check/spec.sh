# shellcheck shell=bash
# Self-tests of scripts/check/spec.sh: spec, gen. Sourced by scripts/test/check.sh, which has the harness.

# --- spec (RAN-15) -------------------------------------------------------------

# spec_fixture: the repo's real contracts, generated code and embedded schema copies, and the
# installed TypeScript tools that generate shared/ts/gen, with the Node and pnpm pins.
spec_fixture() {
  mkdir -p "${REPO}/services/runner/internal/session" "${REPO}/shared/go" "${REPO}/shared/ts"
  cp -R "${ROOT}/api" "${REPO}/api"
  cp "${ROOT}/go.mod" "${ROOT}/go.sum" "${ROOT}/package.json" "${ROOT}/.nvmrc" "${REPO}/"
  ln -s "${ROOT}/node_modules" "${REPO}/node_modules"
  cp -R "${ROOT}/shared/go/gen" "${REPO}/shared/go/"
  cp -R "${ROOT}/shared/ts/gen" "${REPO}/shared/ts/"
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

test_spec_fails_on_stale_typescript_types() {
  spec_fixture
  printf '// edited by hand\n' >>"${REPO}/shared/ts/gen/agent-event.v1alpha1.schema.d.ts"
  run spec
  expect_status 1
  expect_out "shared/ts/gen is stale; run scripts/check.sh gen"
  expect_out "FAIL: spec (generated types)"
}

test_gen_writes_the_typescript_types() {
  spec_fixture
  rm -r "${REPO}/shared/ts/gen"
  run gen
  expect_status 0
  diff -r "${ROOT}/shared/ts/gen" "${REPO}/shared/ts/gen" >/dev/null || fail "gen output differs from shared/ts/gen"
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
