#!/usr/bin/env bash
# Self-tests of the git mirror (RAN-17), run by `script/check.sh hooks`. Every request takes the
# real path: smart HTTP -> backend.sh (CGI) -> git http-backend -> the hooks. The suite runs twice:
#   cgi:   refs_test_server.py stands in for nginx + fcgiwrap + the runner's /auth/git;
#   nginx: real nginx (nginx.conf, rendered for the test) + fcgiwrap, with a stub /auth/git.
# Without nginx and fcgiwrap the second run is skipped, unless RANGELIGHT_NGINX_REQUIRED=true (CI).
# The stub identity is the X-Stub-User header: a, b, r (runner), deny (401), or passed on as it is.
# The cases are in refs_test_cases.sh.
set -euo pipefail
shopt -s inherit_errexit
export LC_ALL=C

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TMP="$(mktemp -d /tmp/rl-git-mirror-test.XXXXXX)"
PIDS=()
FAILS=0
MODE=setup
ZERO=0000000000000000000000000000000000000000

cleanup() {
  local pid
  # A negative pid is a whole process group: fcgiwrap's workers would outlive (and be respawned by) it.
  for pid in "${PIDS[@]}"; do kill -- "${pid}" 2>/dev/null || true; done
  for pid in "${PIDS[@]}"; do wait "${pid#-}" 2>/dev/null || true; done
  rm -rf "${TMP}"
}
trap cleanup EXIT

# The client side never reads the developer's own git config.
export HOME="${TMP}" GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL="${TMP}/gitconfig" GIT_TERMINAL_PROMPT=0
# A stalled HTTP transfer fails within seconds instead of hanging until CI cancels the job.
export GIT_HTTP_LOW_SPEED_LIMIT=1 GIT_HTTP_LOW_SPEED_TIME=15
git config --global user.name test
git config --global user.email test@example.invalid
git config --global init.defaultBranch main
git config --global advice.detachedHead false

pass() { printf 'ok   [%s] %s\n' "${MODE}" "$1"; }
fail() {
  printf 'FAIL [%s] %s\n' "${MODE}" "$1" >&2
  [[ ! -s "${TMP}/out" ]] || sed 's/^/     | /' "${TMP}/out" >&2
  FAILS=$((FAILS + 1))
}
# check <name> <bash condition>
check() { if eval "$2"; then pass "$1"; else fail "$1"; fi; }

# ok <name> <command...>: the command succeeds.
ok() {
  local name="$1"
  shift
  if "$@" >"${TMP}/out" 2>&1; then pass "${name}"; else fail "${name} (failed)"; fi
}

# refused <name> <reason> <command...>: the command fails, and says why.
refused() {
  local name="$1" reason="$2"
  shift 2
  if "$@" >"${TMP}/out" 2>&1; then
    fail "${name} (succeeded)"
  elif grep -qaF -e "${reason}" "${TMP}/out"; then
    pass "${name}"
  else
    fail "${name} (no '${reason}')"
  fi
}

# client_ip <user>: the address that user's requests come from, for nginx's per-address limits
# (in the nginx run, the rendered nginx.conf takes it from X-Test-Client-IP; see start_nginx).
client_ip() {
  case "$1" in
    a) printf 10.0.0.1 ;;
    b) printf 10.0.0.2 ;;
    r) printf 10.0.0.3 ;;
    *) printf 10.0.0.9 ;;
  esac
}

# as <user> <git args...>: git against the mirror as that identity ("-" sends none).
as() {
  local user="$1" ip
  shift
  ip="$(client_ip "${user}")"
  if [[ "${user}" == - ]]; then
    git -c "http.extraHeader=X-Test-Client-IP: ${ip}" "$@"
  else
    git -c "http.extraHeader=X-Stub-User: ${user}" -c "http.extraHeader=X-Test-Client-IP: ${ip}" "$@"
  fi
}

# http <user> <path>: GET it; prints the status, then the body.
http() { curl -s -H "X-Stub-User: $1" -H "X-Test-Client-IP: $(client_ip "$1")" -w '\n%{http_code}\n' "${BASE}$2" | tac; }

ref_in() { git -C "${SRV}/$1" rev-parse -q --verify "$2"; }

commit_in() {
  printf '%s\n' "$2" >"$1/$2.txt"
  git -C "$1" add "$2.txt"
  git -C "$1" commit -qm "$2"
}

# --- the two front ends ---------------------------------------------------------------

wait_for() {
  local _
  for _ in $(seq 1 100); do eval "$1" && return 0; sleep 0.05; done
  printf 'the test front end did not start (%s)\n' "$1" >&2
  cat "${W}"/*.log >&2 || true
  exit 1
}

start_stub() {
  rm -f "${W}/port"
  python3 -I "${HERE}/refs_test_server.py" "$@" 2>>"${W}/stub.log" &
  PIDS+=($!)
  wait_for "[[ -s '${W}/port' ]]"
}

start_cgi() {
  start_stub cgi "${HERE}/backend.sh" "${SRV}" "${W}/port" "${W}"
  BASE="http://127.0.0.1:$(<"${W}/port")"
}

free_port() { python3 -I -c 'import socket; s = socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1])'; }

start_nginx() {
  local ng="${W}/ng" port auth
  start_stub auth "${W}/port" "${W}"
  auth="$(<"${W}/port")" port="$(free_port)"
  mkdir -p "${ng}/fcgiwrap"
  : >"${ng}/resolver.conf"
  sed -e "s|/tmp/|${ng}/|g" -e "s|/opt/git-mirror/|${HERE}/|g" -e "s|/srv/git|${SRV}|g" \
    -e "s|listen 8080;|listen 127.0.0.1:${port}; set_real_ip_from 127.0.0.1; real_ip_header X-Test-Client-IP;|" \
    -e "s|http://runner.rangelight-runner.svc.cluster.local:8080/|http://127.0.0.1:${auth}/|" \
    -e "s|# No checkpoint receiver yet.*|fastcgi_param RANGELIGHT_EVENTS_URL http://127.0.0.1:${auth}/events;|" \
    "${HERE}/nginx.conf" >"${ng}/nginx.conf"
  check "nginx.conf was fully rendered for the test" \
    "! grep -qE '/srv/git|/opt/|cluster\.local|[ :]/tmp/(nginx|fcgiwrap|resolver)' '${ng}/nginx.conf' && grep -q RANGELIGHT_EVENTS_URL '${ng}/nginx.conf'"
  ok "nginx -t accepts nginx.conf" nginx -e stderr -p "${ng}" -c "${ng}/nginx.conf" -t
  # Its own process group (setsid), so cleanup can stop the workers with it.
  setsid fcgiwrap -c 16 -s "unix:${ng}/fcgiwrap/fcgiwrap.sock" 2>>"${W}/fcgiwrap.log" &
  PIDS+=("-$!")
  # nginx answers /healthz on its own: without fcgiwrap's socket, the first git request gets a 502.
  wait_for "[[ -S '${ng}/fcgiwrap/fcgiwrap.sock' ]]"
  nginx -e stderr -p "${ng}" -c "${ng}/nginx.conf" -g 'daemon off;' >>"${W}/nginx.log" 2>&1 &
  PIDS+=($!)
  BASE="http://127.0.0.1:${port}"
  wait_for "curl -sf '${BASE}/healthz' >/dev/null"
}

# --- fixtures -----------------------------------------------------------------------

setup() {
  W="${TMP}/${MODE}" SRV="${TMP}/${MODE}/srv"
  mkdir -p "${SRV}"
  git init -q "${W}/upstream"
  commit_in "${W}/upstream" base
  git -C "${W}/upstream" tag v1
  git -C "${W}/upstream" branch old
  env GIT_ALLOW_PROTOCOL=file "${HERE}/sync.sh" "${SRV}" "demo=file://${W}/upstream"
  "start_${MODE}"
  MAIN="${BASE}/demo.git" SELF="${BASE}/session/demo.git"
  as a clone -q "${MAIN}" "${W}/a"
  as b clone -q "${MAIN}" "${W}/b"
  commit_in "${W}/a" secret-of-a
  commit_in "${W}/b" work-of-b
  SECRET="$(git -C "${W}/a" rev-parse HEAD)"
}

# shellcheck source=services/git-mirror/refs_test_cases.sh
source "${HERE}/refs_test_cases.sh"

suite() {
  MODE="$1"
  setup
  t8_t9_identity_fails_closed
  [[ "${MODE}" != nginx ]] || t8_t9_git_cli_signs_in_after_the_challenge
  [[ "${MODE}" != cgi ]] || t8_t9_hook_fails_closed_on_its_own
  t8_t9_ac1_session_pushes_into_its_own_repo
  t8_t9_ac2_session_cannot_push_main
  t8_t9_checkpoints_are_append_only
  test_push_is_capped_and_fscked
  t8_t9_ac3_session_cannot_push_another_sessions_refs
  t8_t9_ac4_session_cannot_read_another_session
  t8_t9_foreign_object_id_push_yields_nothing
  t8_t9_ac5_runner_reads_and_never_pushes
  test_big_push_goes_chunked
  test_ac6_sync_keeps_session_refs
  [[ "${MODE}" != nginx ]] || test_limits_hold_back_one_identity_and_one_address
}

main() {
  suite cgi
  if command -v nginx >/dev/null && command -v fcgiwrap >/dev/null; then
    suite nginx
  elif [[ "${RANGELIGHT_NGINX_REQUIRED:-}" == true ]]; then
    printf 'FAIL nginx and fcgiwrap are required here (RANGELIGHT_NGINX_REQUIRED=true)\n' >&2
    FAILS=$((FAILS + 1))
  else
    printf 'skipped: git-mirror through real nginx (nginx or fcgiwrap not installed)\n'
  fi
  [[ "${FAILS}" -eq 0 ]] || { printf 'git-mirror: %d test(s) failed\n' "${FAILS}" >&2; exit 1; }
  printf 'git-mirror: all tests passed\n'
}

main "$@"
