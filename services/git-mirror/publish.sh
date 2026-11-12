#!/usr/bin/env bash
# The runner's publish Job (ADR 0011: the push upstream is a control-plane job, never the mirror).
# Copies one session's work branch, and nothing else, from the git mirror to the repo's upstream.
# Usage: publish.sh <session id> <mirror URL> <upstream URL>
# The two credentials are files: the Job's projected token for the mirror (the runner's identity
# there, read-only) and the GitHub App's installation token. Each goes only into an Authorization
# header for its own host, through git's environment config: never argv, a URL, or the log.
# Exit 0: published, or nothing to publish (the session's repo has no work branch); 3: GitHub refuses it for good (a workflow file changed,
# and the App has no workflows permission); anything else: a failure the Job retries.
# The last line of the termination log says which, for the runner.
set -euo pipefail
export LC_ALL=C GIT_TERMINAL_PROMPT=0 GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null
export GIT_ALLOW_PROTOCOL="${GIT_ALLOW_PROTOCOL:-http:https}"

log="${RANGELIGHT_TERMINATION_LOG:-/dev/termination-log}"
say() {
  printf '%s\n' "$1" >"${log}"
  printf 'publish: %s\n' "$1" >&2
}
[[ $# -eq 3 && "$1" =~ ^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$ ]] ||
  { say "usage: publish.sh <session id> <mirror URL> <upstream URL>"; exit 2; }
id="$1" mirror="$2" upstream="$3"
ref="refs/heads/rangelight/${id}/work"

# basic <user> <token file>: the Basic credentials, the token's trailing newline dropped.
basic() { printf '%s:%s' "$1" "$(tr -d '\n' <"$2")" | base64 | tr -d '\n'; }
mirror_auth="$(basic runner "${RANGELIGHT_MIRROR_TOKEN_FILE:-/var/run/secrets/rangelight/git-mirror/token}")"
github_auth="$(basic x-access-token "${RANGELIGHT_GITHUB_TOKEN_FILE:-/var/run/rangelight/publish/token}")"
export GIT_CONFIG_COUNT=2 \
  GIT_CONFIG_KEY_0="http.${mirror}/.extraHeader" GIT_CONFIG_VALUE_0="Authorization: Basic ${mirror_auth}" \
  GIT_CONFIG_KEY_1="http.${upstream}.extraHeader" GIT_CONFIG_VALUE_1="Authorization: Basic ${github_auth}"

GIT_DIR="$(mktemp -d)/work.git"
export GIT_DIR
git init -q --bare "${GIT_DIR}"
err="${GIT_DIR}/err"

if ! git fetch -q --no-tags "${mirror}/sessions/${id}.git" "+${ref}:${ref}" 2>"${err}"; then
  # Only a missing ref is "nothing": a missing repo, a 404 or a 5xx from the mirror is a failure.
  if grep -qF "couldn't find remote ref ${ref}" "${err}"; then
    say "nothing to publish"
    exit 0
  fi
  say "fetching the work branch from the mirror failed: $(tail -n 2 "${err}" | tr '\n' ' ')"
  exit 1
fi
if ! git push -q "${upstream}" "+${ref}:${ref}" 2>"${err}"; then
  # shellcheck disable=SC2016 # GitHub's message, backticks and all
  if grep -qF 'without `workflows` permission' "${err}"; then
    say "GitHub refused it: the work branch changes .github/workflows, which the GitHub App may not push"
    exit 3
  fi
  say "pushing the work branch failed: $(tail -n 2 "${err}" | tr '\n' ' ')"
  exit 1
fi
say "published $(git rev-parse "${ref}")"
