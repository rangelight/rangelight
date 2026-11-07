#!/usr/bin/env bash
# A stand-in for the `claude` binary in the cli runtime's tests: the same flags, the same HTTP
# hooks, no model. In $CLAUDE_CONFIG_DIR:
# - `auth status` succeeds when signed-in exists;
# - each launch appends its arguments, joined by \x1f, as one line of launches;
# - `--session-id <id>` creates sessions/<id>; `--resume <id>` exits 1 without it, as claude does.
# A turn posts UserPromptSubmit, PostToolUse (the tool is the prompt's first word) and Stop. A
# prompt with "slow" waits 400 ms for Escape, which cancels the turn; like Claude Code's terminal
# input, an Escape followed within 50 ms by another byte is Alt+<byte>, not Escape. The prompt
# "exit" exits with 3.
set -euo pipefail

home="${CLAUDE_CONFIG_DIR:?}"
if [[ "${1-}" == auth ]]; then
  [[ -e "${home}/signed-in" ]]
  exit
fi
(IFS=$'\x1f' && printf '%s\n' "$*") >>"${home}/launches"

settings=""
args=("$@")
for i in "${!args[@]}"; do
  case "${args[i]}" in
    --settings) settings="${args[i + 1]}" ;;
    --session-id) mkdir -p "${home}/sessions" && touch "${home}/sessions/${args[i + 1]}" ;;
    --resume) [[ -e "${home}/sessions/${args[i + 1]}" ]] || exit 1 ;;
  esac
done
[[ "${settings}" =~ \"url\":\"([^\"]+)\" ]]
url="${BASH_REMATCH[1]}"

post() { curl -fsS -o /dev/null -X POST --data-binary "$1" "${url}"; }

# key [seconds]: one byte from the PTY into $key, as Claude Code reads it in raw mode. Not bash's
# `read -n`, which switches the terminal back to turning Enter into a newline. --foreground keeps
# head in the terminal's foreground group, where it may read.
key() {
  key="$(timeout --foreground "${1:-infinity}" head -c1 && printf x)" || return 1
  key="${key%x}"
}

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

stty raw -echo
turn "${*: -1}"
line=""
while key; do
  case "${key}" in
    $'\r') turn "${line}" && line="" ;;
    $'\e') ;;
    *) line+="${key}" ;;
  esac
done
