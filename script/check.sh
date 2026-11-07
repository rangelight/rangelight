#!/usr/bin/env bash
# The whole local gate, and exactly what CI runs (ADR 0049, amendment 2026-11-07).
# Usage: script/check.sh <part>    Run without a part to list them.
set -euo pipefail
shopt -s inherit_errexit

ROOT="$(git rev-parse --show-toplevel)"
PARTS=(size rules secrets scrub spec rust go ts deploy hooks fmt all)
GOLANGCI_LINT="github.com/golangci/golangci-lint/v2/cmd/golangci-lint@v2.15.0"
GOVULNCHECK="golang.org/x/vuln/cmd/govulncheck@v1.9.0"
# Built from parts so the rules part never matches its own source.
TAG="TO""DO"

# --- helpers -------------------------------------------------------------------

# Every file in the tree that git doesn't ignore: tracked, plus new files not yet added.
repo_files() {
  git -c core.quotePath=false ls-files --cached --others --exclude-standard | sort -u |
    while IFS= read -r f; do [[ -f "${f}" ]] && printf '%s\n' "${f}"; done
}

has_files() { repo_files | grep -qE "$1"; }

need() {
  local tool
  for tool in "$@"; do
    command -v "${tool}" >/dev/null || { printf 'missing tool: %s\n' "${tool}" >&2; return 1; }
  done
}

ok() { printf 'ok: %s\n' "$1"; }
skip() { printf 'skipped: %s (no sources yet)\n' "$1"; }
failed() { printf 'FAIL: %s\n' "$1" >&2; return 1; }

# listed <path> <file>: is the path a line of the file (ignoring # comments)?
listed() { [[ -f "$2" ]] && grep -v '^#' "$2" | grep -qxF -e "$1"; }

# located <rule>: turn grep's file:line:content into file:line: rule.
located() { awk -F: -v rule="$1" '{ print $1 ":" $2 ": " rule }'; }

# scan <rule> <ere> <files...>: print file:line for each match, without the content.
scan() {
  local rule="$1" re="$2"
  shift 2
  [[ $# -gt 0 ]] || return 0
  { grep -nHE -e "${re}" -- "$@" || true; } | located "${rule}"
}

# --- size (ADR 0049 §7) --------------------------------------------------------

size_counted() {
  case "$1" in
    *.rs | *.go | *.ts | *.tsx | *.sh | *.sql) ;;
    *) return 1 ;;
  esac
  case "/$1" in
    */tests/* | */gen/* | */rl-proto/* | */.sqlx/* | */tests.rs | *_test.go | *.test.ts) return 1 ;;
  esac
}

size() {
  local f n limit label bad=0
  while IFS= read -r f; do
    size_counted "${f}" || continue
    listed "${f}" script/size-exceptions.txt && continue
    n="$(awk 'END { print NR }' "${f}")"
    limit=400
    label="limit"
    case "${f##*/}" in
      main.rs | main.go | main.ts) limit=80 label="entrypoint limit" ;;
    esac
    if ((n > limit)); then
      printf '%s: %d lines (%s %d)\n' "${f}" "${n}" "${label}" "${limit}"
      bad=1
    fi
  done < <(repo_files)
  [[ "${bad}" -eq 0 ]] || failed size
  ok size
}

# --- rules (ADR 0049 §2.4, §5.1, §6, §10) --------------------------------------

ENV_RE='\benv::vars?(_os)?\(|\bos\.(Getenv|LookupEnv|Environ)\(|\bprocess\.env\b|\bimport\.meta\.env\b'
SQL_RE='\bquery(_as|_scalar)?!\(|\bquery(_as|_scalar)?(::<[^>]*>)?\([[:space:]]*r?#*"'
COLOUR_RE='#([0-9a-fA-F]{3,4}|[0-9a-fA-F]{6}|[0-9a-fA-F]{8})\b|\b(rgba?|hsla?)\('

rules() {
  local f out env=() sql=() todo=() colour=()
  while IFS= read -r f; do
    case "${f}" in
      control-plane/crates/rl-config/* | runner/internal/config/* | agent/src/config.ts) ;;
      *.rs | *.go | *.ts | *.tsx) env+=("${f}") ;;
    esac
    case "${f}" in *.rs) sql+=("${f}") ;; esac
    case "${f}" in
      docs/architecture/adr/*) ;;
      *.rs | *.go | *.ts | *.tsx | *.js | *.sh | *.sql | *.toml | *.yml | *.yaml | *.css | *.proto) todo+=("${f}") ;;
    esac
    case "${f}" in
      web/src/styles/tokens.css) ;;
      web/*.ts | web/*.tsx | web/*.css | web/*.html) colour+=("${f}") ;;
    esac
  done < <(repo_files)
  out="$(
    scan "environment read outside a config module (ADR 0049 §6)" "${ENV_RE}" "${env[@]}"
    scan "SQL in a Rust string literal; use queries/*.sql (ADR 0049 §2.4)" "${SQL_RE}" "${sql[@]}"
    if [[ ${#todo[@]} -gt 0 ]]; then
      { grep -nHE -e "\b${TAG}\b" -- "${todo[@]}" || true; } |
        sed -E "s/${TAG}\(RAN-[1-9][0-9]*\)//g" | { grep -E "\b${TAG}\b" || true; } |
        located "${TAG} without a ticket, write ${TAG}(RAN-<n>) (ADR 0049 §10)"
    fi
    scan "raw colour outside web/src/styles/tokens.css (ADR 0049 §5.1)" "${COLOUR_RE}" "${colour[@]}"
  )"
  if [[ -n "${out}" ]]; then
    printf '%s\n' "${out}"
    failed rules
  fi
  ok rules
}

# --- secrets (ADR 0050 §7) -----------------------------------------------------

secrets() {
  need gitleaks tar
  local tmp
  tmp="$(mktemp -d /tmp/rl-check-secrets.XXXXXX)"
  # shellcheck disable=SC2064 # expand now: tmp is local
  trap "rm -rf '${tmp}'" EXIT
  # The working tree as git sees it (ignored files left out), including uncommitted work.
  repo_files | tar -cf - --verbatim-files-from -T - | tar -xf - -C "${tmp}"
  (cd "${tmp}" && gitleaks dir --no-banner --redact --exit-code 1 .) || failed secrets
  if git rev-parse -q --verify HEAD >/dev/null; then
    gitleaks git --no-banner --redact --exit-code 1 . || failed secrets
  fi
  ok secrets
}

# --- scrub: no private names in the public repo (ADR 0026) ----------------------

scrub() {
  local pattern="${RANGELIGHT_SCRUB_PATTERN:-}" hit file rest n line bad=0
  if [[ -z "${pattern}" ]]; then
    if [[ "${RANGELIGHT_SCRUB_REQUIRED:-}" == "true" ]]; then
      printf 'RANGELIGHT_SCRUB_PATTERN is not set, and scrub is required here\n' >&2
      failed scrub
    fi
    printf 'skipped: scrub (no pattern)\n'
    return 0
  fi
  # A broken pattern is an error (exit 2), not a clean tree.
  if ! { grep -qiE -e "${pattern}" </dev/null || [[ $? -eq 1 ]]; }; then
    failed "scrub (invalid pattern)"
  fi
  while IFS= read -r hit; do
    file="${hit%%:*}"
    rest="${hit#*:}"
    n="${rest%%:*}"
    line="${rest#*:}"
    listed "${line}" script/scrub-allow.txt && continue
    # Location only: the matched text would publish what the scrub protects.
    printf '%s:%s\n' "${file}" "${n}"
    bad=1
  done < <(repo_files | xargs -r -d '\n' grep -nHIiE -e "${pattern}" -- || true)
  [[ "${bad}" -eq 0 ]] || failed scrub
  ok scrub
}

# --- per-component parts --------------------------------------------------------

spec() {
  has_files '^spec/.*\.(proto|json|yaml)$' || { skip spec; return 0; }
  need buf
  local base="${RANGELIGHT_SPEC_BASE:-origin/main}"
  (cd spec && buf format --diff --exit-code && buf lint)
  if git cat-file -e "${base}:spec" 2>/dev/null; then
    (cd spec && buf breaking --against "${ROOT}/.git#ref=${base},subdir=spec")
  else
    printf 'spec: %s has no spec/ yet, nothing to break\n' "${base}"
  fi
  ok spec
}

rust() {
  has_files '^control-plane/.*\.rs$' || { skip rust; return 0; }
  need cargo cargo-nextest cargo-deny cargo-machete
  (
    cd control-plane
    cargo fmt --all --check
    cargo clippy --workspace --all-targets --locked -- -D warnings
    cargo nextest run --workspace --locked --no-tests=pass
    cargo deny check
    cargo machete
  )
  ok rust
}

go() {
  has_files '^runner/.*\.go$' || { skip go; return 0; }
  need go
  (
    cd runner
    command go run "${GOLANGCI_LINT}" run ./...
    command go test -race ./...
    command go run "${GOVULNCHECK}" ./...
  )
  ok go
}

ts() {
  has_files '^(agent|web)/.*\.tsx?$' || { skip ts; return 0; }
  need pnpm
  pnpm install --frozen-lockfile
  pnpm exec prettier --check .
  pnpm exec eslint --max-warnings 0 .
  pnpm -r --if-present run typecheck
  pnpm -r --if-present run test
  pnpm audit --prod
  ok ts
}

deploy() {
  need shellcheck
  local scripts=() t
  mapfile -t scripts < <(repo_files | grep -E '\.sh$|^git-mirror/hooks/[^/]+$')
  shellcheck -x -- "${scripts[@]}"
  if has_files '^charts/rangelight/Chart\.yaml$'; then
    need helm kubeconform
    helm lint --strict charts/rangelight
    helm template rangelight charts/rangelight | kubeconform -strict -summary
  fi
  for t in script/test/*.sh; do bash "${t}"; done
  ok deploy
}

hooks() {
  has_files '^git-mirror/' || { skip hooks; return 0; }
  local tests=() t
  mapfile -t tests < <(repo_files | grep -E '^git-mirror/test/[^/]+\.sh$')
  [[ ${#tests[@]} -gt 0 ]] || { printf 'git-mirror/ has no tests in git-mirror/test/\n' >&2; failed hooks; }
  for t in "${tests[@]}"; do bash "${t}"; done
  ok hooks
}

# Rewrites files in place; not part of `all` (each part checks formatting itself).
fmt() {
  local did=0
  if has_files '^control-plane/.*\.rs$'; then (cd control-plane && cargo fmt --all); did=1; fi
  if has_files '^runner/.*\.go$'; then (cd runner && command go run "${GOLANGCI_LINT}" fmt); did=1; fi
  if has_files '^(agent|web)/.*\.tsx?$'; then pnpm exec prettier --write .; did=1; fi
  [[ "${did}" -eq 1 ]] || { skip fmt; return 0; }
  ok fmt
}

all() {
  local part bad=()
  for part in size rules secrets scrub spec rust go ts deploy hooks; do
    bash "${ROOT}/script/check.sh" "${part}" || bad+=("${part}")
  done
  [[ ${#bad[@]} -eq 0 ]] || failed "all (${bad[*]})"
  ok all
}

# --- dispatcher -----------------------------------------------------------------

usage() {
  printf 'usage: script/check.sh <part>\nparts: %s\n' "${PARTS[*]}" >&2
}

main() {
  local part="${1:-}"
  if [[ " ${PARTS[*]} " != *" ${part} "* ]] || ! declare -f "${part}" >/dev/null; then
    usage
    exit 2
  fi
  shift
  cd "${ROOT}"
  "${part}" "$@"
}

main "$@"
