# shellcheck shell=bash
# Self-tests of scripts/check/code.sh: yaml, hooks. Sourced by scripts/test/check.sh, which has the harness.

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

# --- hooks (RAN-17): needs a test, runs it, and finds it in a large tree (grep -q + pipefail) ---
test_hooks_needs_and_runs_the_git_mirror_tests() {
  put services/git-mirror/hooks/pre-receive 'exit 1'; run hooks; expect_status 1; expect_out "has no *_test.sh"
  put services/git-mirror/refs_test.sh 'echo ran-refs-test; exit 1'; run hooks; expect_status 1; expect_out "ran-refs-test"
  put services/git-mirror/refs_test.sh 'echo ran-refs-test'
  for i in $(seq 1 500); do put "z/file-${i}.txt" x; done
  run hooks; expect_status 0; expect_out "ok: hooks"
}
