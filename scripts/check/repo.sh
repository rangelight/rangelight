# shellcheck shell=bash
# The repo-wide parts: size, rules, secrets, scrub. Sourced by scripts/check.sh, which holds the helpers, the pins and the dispatcher.

# Built from parts so the rules part never matches its own source.
TAG="TO""DO"

# --- size (ADR 0049 §7) --------------------------------------------------------

size_counted() {
  case "$1" in
    *.rs | *.go | *.ts | *.tsx | *.sh | *.sql) ;;
    *) return 1 ;;
  esac
  case "/$1" in
    */tests/* | */gen/* | */shared/rust/proto/* | */.sqlx/* | */tests.rs | *_test.go | *.test.ts) return 1 ;;
  esac
}

size() {
  local f n limit label bad=0
  while IFS= read -r f; do
    size_counted "${f}" || continue
    listed "${f}" scripts/size-exceptions.txt && continue
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
      shared/rust/config/* | services/runner/internal/config/* | services/agent/src/config.ts) ;;
      *.rs | *.go | *.ts | *.tsx) env+=("${f}") ;;
    esac
    case "${f}" in *.rs) sql+=("${f}") ;; esac
    case "${f}" in
      docs/architecture/adr/*) ;;
      *.rs | *.go | *.ts | *.tsx | *.js | *.sh | *.sql | *.toml | *.yaml | *.css | *.proto) todo+=("${f}") ;;
    esac
    case "${f}" in
      services/web/src/styles/tokens.css) ;;
      services/web/*.ts | services/web/*.tsx | services/web/*.css | services/web/*.html) colour+=("${f}") ;;
    esac
  done < <(repo_files)
  out="$(
    scan "environment read outside a config module (ADR 0049 §6)" "${ENV_RE}" "${env[@]}"
    scan "SQL in a Rust string literal; use db/queries/*.sql (ADR 0049 §2.4)" "${SQL_RE}" "${sql[@]}"
    if [[ ${#todo[@]} -gt 0 ]]; then
      { grep -nHE -e "\b${TAG}\b" -- "${todo[@]}" || true; } |
        sed -E "s/${TAG}\(RAN-[1-9][0-9]*\)//g" | { grep -E "\b${TAG}\b" || true; } |
        located "${TAG} without a ticket, write ${TAG}(RAN-<n>) (ADR 0049 §10)"
    fi
    scan "raw colour outside services/web/src/styles/tokens.css (ADR 0049 §5.1)" "${COLOUR_RE}" "${colour[@]}"
  )"
  if [[ -n "${out}" ]]; then
    printf '%s\n' "${out}"
    failed rules
  fi
  ok rules
}

# --- secrets (ADR 0050 §7) -----------------------------------------------------

secrets() {
  need gitleaks || failed "secrets (tools)"
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
    listed "${line}" scripts/scrub-allow.txt && continue
    # Location only: the matched text would publish what the scrub protects.
    printf '%s:%s\n' "${file}" "${n}"
    bad=1
  done < <(repo_files | xargs -r -d '\n' grep -nHIiE -e "${pattern}" -- || true)
  [[ "${bad}" -eq 0 ]] || failed scrub
  ok scrub
}
