#!/usr/bin/env bash
# A stand-in for the `claude` binary in the cli runtime's tests: the same flags, the same HTTP
# hooks, no model. In $CLAUDE_CONFIG_DIR:
# - `auth status` succeeds when signed-in exists;
# - each launch appends its arguments, joined by \x1f, as one line of launches;
# - `--session-id <id>` creates sessions/<id>; `--resume <id>` exits 1 without it, as claude does.
# A turn posts UserPromptSubmit, PostToolUse (the tool is the prompt's first word) and Stop. A
# prompt with "slow" waits 400 ms for Escape, which cancels the turn; like Claude Code's terminal
# input, an Escape followed within 50 ms by another byte is Alt+<byte>, not Escape. The prompt
# "exit" exits with 3. The line "size" is no turn: it prints the PTY's size and how many SIGWINCH
# it has had, "<rows> <cols> <winches>". Without --session-id or --resume it is the sign-in claude:
# the line "/login" creates signed-in, and there are no turns. Either kind first shows the workspace-
# trust dialog, as Claude Code does in a folder it doesn't trust yet: the line "yes" answers it and
# creates trusted (at once when trusted exists); then it runs its SessionStart hooks as Claude Code
# 2.1.319 does: command hooks only, never an HTTP one. After the prompt
# "stubborn" it ignores SIGTERM and SIGHUP and reads nothing more, so only SIGKILL ends it; its sleep is named
# rl-stubborn-<the Claude home's directory name>.
set -euo pipefail

home="${CLAUDE_CONFIG_DIR:?}"
if [[ "${1-}" == auth ]]; then
  [[ -e "${home}/signed-in" ]]
  exit
fi
(IFS=$'\x1f' && printf '%s\n' "$*") >>"${home}/launches"

winches=0
trap 'winches=$((winches + 1))' WINCH

# key [seconds]: one byte from the PTY into $key, as Claude Code reads it in raw mode. Not bash's
# `read -n`, which switches the terminal back to turning Enter into a newline. --foreground keeps
# head in the terminal's foreground group, where it may read.
key() {
  key="$(timeout --foreground "${1:-infinity}" head -c1 && printf x)" || return 1
  key="${key%x}"
}

settings="" session=""
args=("$@")
for i in "${!args[@]}"; do
  case "${args[i]}" in
    --settings) settings="${args[i + 1]}" ;;
    --session-id) session=1 && mkdir -p "${home}/sessions" && touch "${home}/sessions/${args[i + 1]}" ;;
    --resume) session=1 && { [[ -e "${home}/sessions/${args[i + 1]}" ]] || exit 1; } ;;
  esac
done
url=""
if [[ "${settings}" =~ \"url\":\"([^\"]+)\" ]]; then url="${BASH_REMATCH[1]}"; fi

post() { curl -fsS -o /dev/null -X POST --data-binary "$1" "${url}"; }

# session_start: the settings' SessionStart command hooks, each through sh; HTTP ones are skipped.
session_start() {
  local command
  # shellcheck disable=SC2016 # a JavaScript program, not an expansion
  command="$(node --input-type=module -e 'for (const m of JSON.parse(process.argv[1]).hooks?.SessionStart ?? [])
    for (const h of m.hooks) if (h.type === "command") console.log(h.command);' "${settings}")"
  if [[ -n "${command}" ]]; then sh -c "${command}" </dev/null; fi
}

# trust: the workspace-trust dialog until the line "yes" (none when trusted exists), then SessionStart.
trust() {
  if [[ ! -e "${home}/trusted" ]]; then
    printf 'Do you trust this folder? Type yes\r\n'
    local line=""
    while :; do
      key || exit 0
      if [[ "${key}" != $'\r' ]]; then line+="${key}" && continue; fi
      [[ "${line}" == yes ]] && break
      line=""
    done
    touch "${home}/trusted"
  fi
  session_start
}

stty raw -echo
if [[ -z "${session}" ]]; then
  trust
  printf 'Sign in: type /login\r\n'
  line=""
  while key; do
    case "${key}" in
      $'\r') [[ "${line}" == /login ]] && touch "${home}/signed-in" && printf 'Login successful\r\n'
        line="" ;;
      *) line+="${key}" ;;
    esac
  done
  exit 0
fi
trust

# cancelled: an Escape arrived during the slow wait, on its own.
cancelled() {
  key 0.4 && [[ "${key}" == $'\e' ]] || return 1
  ! key 0.05
}

turn() {
  local tool="${1%% *}"
  tool="${tool//[^A-Za-z0-9_-]/}"
  [[ "$1" == exit ]] && exit 3
  post '{"hook_event_name":"UserPromptSubmit","session_id":"fake"}'
  post "{\"hook_event_name\":\"PostToolUse\",\"session_id\":\"fake\",\"tool_name\":\"${tool:-Bash}\"}"
  if [[ "$1" == *slow* ]] && cancelled; then
    return 0
  fi
  post '{"hook_event_name":"Stop","session_id":"fake","stop_hook_active":false}'
}

turn "${*: -1}"
if [[ "${*: -1}" == stubborn ]]; then
  trap '' TERM HUP
  (exec -a "rl-stubborn-${home##*/}" sleep infinity)
fi
line=""
while key; do
  case "${key}" in
    $'\r') if [[ "${line}" == size ]]; then printf '%s %s\r\n' "$(stty size)" "${winches}"; else turn "${line}"; fi && line="" ;;
    $'\e') ;;
    *) line+="${key}" ;;
  esac
done
