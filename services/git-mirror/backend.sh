#!/usr/bin/env bash
# The CGI program fcgiwrap runs for every request nginx lets through (ADR 0011, RAN-17).
# It maps the caller to the one repo it may use (the table in README.md), then runs stock
# `git http-backend` on it: /<repo>.git (read-only, upstream refs only), /session/<repo>.git (the
# caller's own sessions/<id>.git) or, for the runner only, /sessions/<id>.git (read-only).
# A session repo's path is built only from the validated identity, never from the URL, and its
# objects/info/alternates borrows <repo>.git's objects, so it never reaches another session's.
set -euo pipefail
export LC_ALL=C PATH=/usr/bin:/bin

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=services/git-mirror/identity.sh
source "${HERE}/identity.sh"

refuse() {
  printf 'Status: %s\r\nContent-Type: text/plain\r\n\r\nrangelight: %s\n' "$1" "$2"
  exit 0
}

# Git silently skips a hook it can't execute; then every push would be accepted.
[[ -x "${HERE}/hooks/pre-receive" ]] || refuse "500 Internal Server Error" "the push hook is not executable"
root="${GIT_PROJECT_ROOT-}"
[[ "${root}" == /* && -d "${root}" ]] || refuse "500 Internal Server Error" "no repository root"
rl_identity || refuse "403 Forbidden" "no valid identity"

# The runner drops a closed session's repo (RAN-65): DELETE /sessions/<id>.git, the id a session id
# (the Session CRD's UUIDv7), so the path can only ever be a session repo, never an upstream repo.
if [[ "${REQUEST_METHOD-}" == DELETE ]]; then
  [[ "${RL_ROLE}" == runner ]] || refuse "403 Forbidden" "only the runner removes a session's repo"
  [[ "${PATH_INFO-}" =~ ^/sessions/([0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12})\.git$ ]] ||
    refuse "404 Not Found" "not found"
  dir="${root}/sessions/${BASH_REMATCH[1]}.git"
  [[ -d "${dir}" ]] || refuse "404 Not Found" "no repo sessions/${BASH_REMATCH[1]}.git"
  rm -rf -- "${dir}"
  printf 'Status: 204 No Content\r\n\r\n'
  exit 0
fi

# Smart HTTP only. The dumb protocol (info/refs without ?service=, objects/*, packed-refs)
# would serve files directly instead of through upload-pack.
name='[A-Za-z0-9][A-Za-z0-9._-]*'
[[ "${PATH_INFO-}" =~ ^/(session/|sessions/)?(${name})\.git/(info/refs|git-upload-pack|git-receive-pack)$ ]] ||
  refuse "404 Not Found" "not found"
kind="${BASH_REMATCH[1]}" repo="${BASH_REMATCH[2]}" endpoint="${BASH_REMATCH[3]}" service="${BASH_REMATCH[3]}"
if [[ "${service}" == info/refs ]]; then
  [[ "${QUERY_STRING-}" =~ ^service=git-(upload|receive)-pack$ ]] || refuse "404 Not Found" "not found"
  [[ "${QUERY_STRING}" == *receive* ]] && service=git-receive-pack
fi
push=false
[[ "${service}" == git-receive-pack ]] && push=true

case "${kind}:${RL_ROLE}" in
  ":session" | ":runner")
    [[ "${push}" == false ]] || refuse "403 Forbidden" "${repo}.git is read-only; push to /session/${repo}.git"
    dir="${repo}.git"
    ;;
  "session/:session")
    main="${root}/${repo}.git"
    [[ -d "${main}/objects" ]] || refuse "404 Not Found" "no repo ${repo}.git"
    dir="sessions/${RL_SESSION}.git"
    if [[ ! -d "${root}/${dir}" ]]; then
      [[ "${push}" == true ]] || refuse "404 Not Found" "this session has not pushed yet"
      mkdir -p "${root}/sessions"
      git init -q --bare "${root}/${dir}"
    fi
    alternates="${root}/${dir}/objects/info/alternates"
    if [[ ! -s "${alternates}" ]]; then
      printf '%s\n' "${main}/objects" >"${alternates}.new"
      mv -f "${alternates}.new" "${alternates}"
    fi
    [[ "$(<"${alternates}")" == "${main}/objects" ]] || refuse "403 Forbidden" "this session belongs to another repo"
    ;;
  "sessions/:runner")
    [[ "${push}" == false ]] || refuse "403 Forbidden" "the runner pushes nothing"
    [[ "${repo}" =~ ^${RL_ID_RE}$ ]] || refuse "404 Not Found" "not found"
    dir="sessions/${repo}.git"
    ;;
  *) refuse "403 Forbidden" "not your repo" ;;
esac
[[ -d "${root}/${dir}" ]] || refuse "404 Not Found" "no repo ${dir}"

# fsck every pushed object: a malformed object, or a tree with a .git entry or a dangerous
# .gitmodules, would otherwise reach the runner, which checks session work out later.
config=("core.hooksPath=${HERE}/hooks" "http.receivepack=${push}" "receive.fsckObjects=true")
# Only this config: none from the system, the user, or the caller's environment.
unset GIT_CONFIG_PARAMETERS GIT_CONFIG
export GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_COUNT="${#config[@]}"
for i in "${!config[@]}"; do
  export "GIT_CONFIG_KEY_${i}=${config[i]%%=*}" "GIT_CONFIG_VALUE_${i}=${config[i]#*=}"
done
export PATH_INFO="/${dir}/${endpoint}"
exec git http-backend
