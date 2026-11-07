# shellcheck shell=bash
# The caller's identity, as nginx passes it from the runner's /auth/git (ADR 0011, RAN-19).
# Sourced by backend.sh and the hooks. REMOTE_USER is network input: it is either exactly
# "runner" or "session-<id>". Anything else, including a missing or empty value, is no identity.

# A session id: 1-63 of [a-z0-9-], starting with [a-z0-9]. No "/", no ".", so never a path trick.
RL_ID_RE='[a-z0-9][a-z0-9-]{0,62}'

# rl_identity: sets RL_ROLE (session or runner) and RL_SESSION, or returns 1.
# shellcheck disable=SC2034 # both are read by the scripts that source this file
rl_identity() {
  local LC_ALL=C user="${REMOTE_USER-}"
  RL_ROLE="" RL_SESSION=""
  if [[ "${user}" == runner ]]; then
    RL_ROLE=runner
  elif [[ "${user}" =~ ^session-(${RL_ID_RE})$ ]]; then
    RL_ROLE=session RL_SESSION="${BASH_REMATCH[1]}"
  else
    return 1
  fi
}
