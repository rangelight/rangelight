#!/usr/bin/env bash
# Self-tests for script/metrics.sh, against fake `gh` and `curl` that answer from fixtures.
# No network and no credentials: the Linear key below is a placeholder the fake never checks.
# Usage: script/test/metrics.sh
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel)"
DIR=""
OUT=""
STATUS=0
FAILED=0

setup() {
  DIR="$(mktemp -d /tmp/rl-metrics-test.XXXXXX)"
  mkdir -p "${DIR}/bin" "${DIR}/fx"
  # Ten completed runs of 1..10 minutes.
  jq -n '{ workflow_runs: [range(1; 11) | { run_started_at: "2026-11-02T10:00:00Z",
    updated_at: ("2026-11-02T10:" + (if . < 10 then "0" else "" end) + (. | tostring) + ":00Z") }] }' \
    >"${DIR}/fx/runs_completed.json"
  echo '{ "workflow_runs": [{ "id": 1, "run_attempt": 1 }, { "id": 2, "run_attempt": 2 },
    { "id": 3, "run_attempt": 3 }] }' >"${DIR}/fx/runs_success.json"
  echo '[{ "number": 1, "headRefOid": "aaa" }, { "number": 2, "headRefOid": "bbb" }]' >"${DIR}/fx/prs.json"
  echo '{ "check_runs": [{ "conclusion": "success" }] }' >"${DIR}/fx/checks_aaa.json"
  echo '{ "check_runs": [{ "conclusion": "failure" }] }' >"${DIR}/fx/checks_bbb.json"
  echo '{ "data": { "issues": { "nodes": [
    { "startedAt": "2026-11-01T00:00:00.000Z", "completedAt": "2026-11-02T00:00:00.000Z" },
    { "startedAt": "2026-11-01T00:00:00.000Z", "completedAt": "2026-11-03T00:00:00.000Z" },
    { "startedAt": "2026-11-01T00:00:00.000Z", "completedAt": "2026-11-05T00:00:00.000Z" },
    { "startedAt": null, "completedAt": "2026-11-05T00:00:00.000Z" }] } } }' >"${DIR}/fx/cycle.json"
  echo '{ "data": { "issues": { "nodes": [{ "identifier": "RAN-5" }, { "identifier": "RAN-9" }] } } }' \
    >"${DIR}/fx/owner.json"
  cat >"${DIR}/bin/gh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
expr="." url="" args=("$@")
for ((i = 0; i < ${#args[@]}; i++)); do
  case "${args[i]}" in --jq) expr="${args[i + 1]}" ;; repos/*) url="${args[i]}" ;; esac
done
case "$1 ${url}" in
  "api "*status=completed*) fx=runs_completed ;;
  "api "*status=success*) fx=runs_success ;;
  "api "*/commits/*) fx="checks_$(cut -d/ -f5 <<<"${url}")" ;;
  "pr "*) fx=prs ;;
  *) echo "fake gh: unexpected call: $*" >&2; exit 1 ;;
esac
jq -r "${expr}" "${FIXTURES}/${fx}.json"
EOF
  cat >"${DIR}/bin/curl" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
if grep -q completedAt; then cat "${FIXTURES}/cycle.json"; else cat "${FIXTURES}/owner.json"; fi
EOF
  chmod +x "${DIR}/bin/gh" "${DIR}/bin/curl"
}

# run <command> [env assignments...]
run() {
  local command="$1"
  shift
  STATUS=0
  OUT="$(env -u LINEAR_API_KEY PATH="${DIR}/bin:${PATH}" FIXTURES="${DIR}/fx" "$@" \
    bash "${ROOT}/script/metrics.sh" "${command}" 2>&1)" || STATUS=$?
}

fail() {
  printf '  FAIL: %s\n  --- output ---\n%s\n  --------------\n' "$1" "${OUT}"
  FAILED=1
}

expect_status() { [[ "${STATUS}" -eq "$1" ]] || fail "expected exit $1, got ${STATUS}"; }
expect_out() { [[ "${OUT}" == *"$1"* ]] || fail "expected output to contain: $1"; }

main() {
  setup
  run weekly LINEAR_API_KEY=placeholder
  expect_status 0
  expect_out "ci_p90: 9.0 min (target <= 10)"
  expect_out "red_merges: 1 (target 0) #2"
  expect_out "cycle_time: 2.0 days median"
  expect_out "owner_waits: 2 (target 0) RAN-5 RAN-9"
  expect_out "flaky_reruns: 2 (target 0)"
  run cycle_time
  expect_status 1
  expect_out "LINEAR_API_KEY is not set"
  run nope
  expect_status 2
  expect_out "usage: script/metrics.sh"
  rm -rf "${DIR}"
  if [[ "${FAILED}" -ne 0 ]]; then
    printf 'metrics self-tests: FAILED\n'
    exit 1
  fi
  printf 'metrics self-tests: ok\n'
}

main "$@"
