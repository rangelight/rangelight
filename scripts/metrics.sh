#!/usr/bin/env bash
# The five numbers of the weekly review (ADR 0050 §9), from GitHub and Linear.
# Usage: scripts/metrics.sh <command>
#   weekly                        all five, for the last RANGELIGHT_METRICS_DAYS days (default 7)
#   ci_p90 red_merges cycle_time owner_waits flaky_reruns    one number each
# Needs gh (logged in), jq and curl. The Linear numbers read LINEAR_API_KEY from the environment.
set -euo pipefail
shopt -s inherit_errexit

ROOT="$(git rev-parse --show-toplevel)"
COMMANDS=(weekly ci_p90 red_merges cycle_time owner_waits flaky_reruns)
REPO="${RANGELIGHT_METRICS_REPO:-rangelight/rangelight}"
DAYS="${RANGELIGHT_METRICS_DAYS:-7}"
TEAM="RAN"

# --- helpers -------------------------------------------------------------------

need() {
  local tool
  for tool in "$@"; do
    command -v "${tool}" >/dev/null || { printf 'missing tool: %s\n' "${tool}" >&2; exit 1; }
  done
}

since() { date -u -d "${DAYS} days ago" +%Y-%m-%dT%H:%M:%SZ; }

# The start of the day that is <n> working days (Mon-Fri) before today, UTC.
working_days_ago() {
  local left="$1" day=0
  while ((left > 0)); do
    day=$((day + 1))
    case "$(date -u -d "${day} days ago" +%u)" in 6 | 7) ;; *) left=$((left - 1)) ;; esac
  done
  date -u -d "${day} days ago" +%Y-%m-%dT00:00:00Z
}

# percentile <p>: nearest-rank percentile of the numbers on stdin, or n/a when there are none.
percentile() {
  sort -n | awk -v p="$1" '{ v[NR] = $1 }
    END { if (NR == 0) { print "n/a"; exit } i = int((p * NR + 99) / 100); print v[i < 1 ? 1 : i] }'
}

# linear <graphql query>: POST to Linear's API. The key goes in through a file descriptor, so it
# never appears in the process list, and it is never printed.
linear() {
  [[ -n "${LINEAR_API_KEY:-}" ]] || { printf 'LINEAR_API_KEY is not set\n' >&2; exit 1; }
  jq -n --arg q "$1" '{ query: $q }' |
    curl -sSf https://api.linear.app/graphql -H 'Content-Type: application/json' \
      -H @<(printf 'Authorization: %s\n' "${LINEAR_API_KEY}") --data-binary @-
}

# --- the five numbers ----------------------------------------------------------

# CI p90: wall time of completed `ci` runs, start to finish. Target: 10 minutes or less.
ci_p90() {
  need gh jq
  local p90
  p90="$(gh api --paginate \
    "repos/${REPO}/actions/workflows/ci.yaml/runs?status=completed&created=>=$(since)" \
    --jq '.workflow_runs[] | (.updated_at | fromdate) - (.run_started_at | fromdate)' | percentile 90)"
  if [[ "${p90}" == "n/a" ]]; then
    printf 'ci_p90: n/a (no runs)\n'
  else
    printf 'ci_p90: %s min (target <= 10)\n' "$(awk -v s="${p90}" 'BEGIN { printf "%.1f", s / 60 }')"
  fi
}

# PRs merged while the required `gate` check wasn't green. Target: 0.
red_merges() {
  need gh jq
  local number sha green red=()
  while read -r number sha; do
    green="$(gh api "repos/${REPO}/commits/${sha}/check-runs?check_name=gate" </dev/null \
      --jq '[.check_runs[].conclusion] | any(. == "success")')"
    [[ "${green}" == "true" ]] || red+=("#${number}")
  done < <(gh pr list -R "${REPO}" --state merged --search "merged:>=$(since)" --limit 500 \
    --json number,headRefOid --jq '.[] | "\(.number) \(.headRefOid)"')
  printf 'red_merges: %d (target 0)%s\n' "${#red[@]}" "${red[*]:+ ${red[*]}}"
}

# Median ticket cycle time, In Progress to Done, for tickets completed in the window.
cycle_time() {
  need jq curl
  local median
  median="$(linear "{ issues(first: 250, filter: { team: { key: { eq: \"${TEAM}\" } },
      completedAt: { gte: \"$(since)\" } }) { nodes { startedAt completedAt } } }" |
    jq -r '.data.issues.nodes[] | select(.startedAt != null)
      | ((.completedAt | sub("\\.[0-9]+"; "") | fromdate) - (.startedAt | sub("\\.[0-9]+"; "") | fromdate))' |
    percentile 50)"
  if [[ "${median}" == "n/a" ]]; then
    printf 'cycle_time: n/a (nothing done)\n'
  else
    printf 'cycle_time: %s days median (trend down)\n' "$(awk -v s="${median}" 'BEGIN { printf "%.1f", s / 86400 }')"
  fi
}

# Open `needs-owner` tickets waiting more than 2 working days. Target: 0.
# ponytail: "waiting" is "not updated since"; any comment resets it. Ceiling: a ticket can look
# fresh while still waiting. Upgrade: read the label's add time from the issue history.
owner_waits() {
  need jq curl
  local ids
  ids="$(linear "{ issues(first: 250, filter: { team: { key: { eq: \"${TEAM}\" } },
      labels: { name: { eq: \"needs-owner\" } }, state: { type: { nin: [\"completed\", \"canceled\"] } },
      updatedAt: { lt: \"$(working_days_ago 2)\" } }) { nodes { identifier } } }" |
    jq -r '[.data.issues.nodes[].identifier] | join(" ")')"
  printf 'owner_waits: %d (target 0)%s\n' "$(wc -w <<<"${ids}")" "${ids:+ ${ids}}"
}

# Flaky tests not quarantined within a day. Target: 0.
# ponytail: counts `ci` runs that went green only on a re-run (rerun-until-green, which ADR 0050 §2
# forbids). Ceiling: a flake nobody re-ran is not seen. Upgrade: per-test results from JUnit.
flaky_reruns() {
  need gh jq
  local n
  n="$(gh api --paginate \
    "repos/${REPO}/actions/workflows/ci.yaml/runs?status=success&created=>=$(since)" \
    --jq '.workflow_runs[] | select(.run_attempt > 1) | .id' | wc -l)"
  printf 'flaky_reruns: %d (target 0)\n' "${n}"
}

weekly() {
  printf 'Rangelight weekly metrics, %s to now, %s\n' "$(since)" "${REPO}"
  ci_p90
  red_merges
  cycle_time
  owner_waits
  flaky_reruns
}

# --- dispatcher -----------------------------------------------------------------

usage() {
  printf 'usage: scripts/metrics.sh <command>\ncommands: %s\n' "${COMMANDS[*]}" >&2
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
