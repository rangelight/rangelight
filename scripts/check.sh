#!/usr/bin/env bash
# The whole local gate, and exactly what CI runs (ADR 0049, amendment 2026-11-07).
# Usage: scripts/check.sh <part>    Run without a part to list them.
set -euo pipefail
shopt -s inherit_errexit

ROOT="$(git rev-parse --show-toplevel)"
PARTS=(size rules secrets scrub spec yaml rust go ts deploy hooks fmt all)
GOLANGCI_LINT="github.com/golangci/golangci-lint/v2/cmd/golangci-lint@v2.15.0"
GOVULNCHECK="golang.org/x/vuln/cmd/govulncheck@v1.9.0"
BUF_VERSION="1.75.0"
CHECK_JSONSCHEMA_VERSION="0.38.2"
YAMLLINT_VERSION="1.38.0"
CHART="deployments/helm/rangelight"
# Built from parts so the rules part never matches its own source.
TAG="TO""DO"

# --- helpers -------------------------------------------------------------------

# Every file in the tree that git doesn't ignore: tracked, plus new files not yet added.
repo_files() {
  git -c core.quotePath=false ls-files --cached --others --exclude-standard | sort -u |
    while IFS= read -r f; do [[ -f "${f}" ]] && printf '%s\n' "${f}"; done
}

# Reads all of its input: `grep -q` would exit early and, under pipefail, SIGPIPE reads as "no files".
has_files() { repo_files | grep -E "$1" >/dev/null; }

need() {
  local tool
  for tool in "$@"; do
    command -v "${tool}" >/dev/null || { printf 'missing tool: %s\n' "${tool}" >&2; return 1; }
  done
}

# The toolchain pinned in go.mod. `go run tool@version` builds the tool with the tool's own go line
# and an exported GOTOOLCHAIN wins over go.mod, so every go call here names this one.
go_toolchain() { awk '$1 == "toolchain" { print $2 }' go.mod; }

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
    listed "${line}" scripts/scrub-allow.txt && continue
    # Location only: the matched text would publish what the scrub protects.
    printf '%s:%s\n' "${file}" "${n}"
    bad=1
  done < <(repo_files | xargs -r -d '\n' grep -nHIiE -e "${pattern}" -- || true)
  [[ "${bad}" -eq 0 ]] || failed scrub
  ok scrub
}

# --- spec: the v1alpha1 contracts (ADR 0025, ADR 0027, ADR 0049 §2.3) ----------

SPEC_REQUIRED=(
  api/README.md api/buf.yaml api/buf.gen.yaml
  api/proto/rangelight/runner/v1alpha1/runner.proto
  api/jsonschema/receipt-predicate.v1alpha1.schema.json
  api/jsonschema/rangelight.v1alpha1.schema.json
  api/jsonschema/agent-event.v1alpha1.schema.json
  go.mod go.sum
  services/runner/internal/session/schema/rangelight.v1alpha1.schema.json
)
# The generated package lives where Go imports it from; buf.gen.yaml writes it there.
SPEC_GO_GEN="shared/go/gen"

# Every file the spec needs is there.
spec_files() {
  local f bad=0
  for f in "${SPEC_REQUIRED[@]}"; do
    [[ -f "${f}" ]] || { printf 'spec: missing %s\n' "${f}" >&2; bad=1; }
  done
  [[ "${bad}" -eq 0 ]] || failed "spec (missing files)"
}

# The runner embeds copies of schemas (go:embed can't reach api/); each must equal its source.
spec_copies() {
  local f bad=0
  for f in services/runner/internal/session/schema/*.json; do
    cmp -s "${f}" "api/jsonschema/${f##*/}" && continue
    printf 'spec: %s differs from api/jsonschema/%s; copy it over\n' "${f}" "${f##*/}" >&2
    bad=1
  done
  [[ "${bad}" -eq 0 ]] || failed "spec (embedded copy differs)"
}

# spec_schema_for <example>: the schema an example is written against, from its name's first word.
spec_schema_for() {
  local name="${1##*/}"
  case "${name%%.*}" in
    receipt) printf 'api/jsonschema/receipt-predicate.v1alpha1.schema.json' ;;
    rangelight | agent-event) printf 'api/jsonschema/%s.v1alpha1.schema.json' "${name%%.*}" ;;
    *) return 1 ;;
  esac
}

# Schemas are valid against their metaschema; valid examples pass; every invalid example fails.
spec_schemas() {
  local f schema status bad=0 covered=()
  for f in api/jsonschema/*.json; do
    check-jsonschema --check-metaschema "${f}" >/dev/null || { printf 'spec: %s: not a valid schema\n' "${f}" >&2; bad=1; }
  done
  while IFS= read -r f; do
    schema="$(spec_schema_for "${f}")" || { printf 'spec: %s: no schema for this name\n' "${f}" >&2; bad=1; continue; }
    status=0
    check-jsonschema --schemafile "${schema}" "${f}" >/dev/null 2>&1 || status=$?
    case "${f}:${status}" in
      api/examples/invalid/*:1) covered+=("invalid:${schema}") ;;
      api/examples/invalid/*:0) printf 'spec: %s passes %s; it must fail\n' "${f}" "${schema}" >&2; bad=1 ;;
      *:0) covered+=("valid:${schema}") ;;
      *) check-jsonschema --schemafile "${schema}" "${f}" >&2 || true; bad=1 ;;
    esac
  done < <(repo_files | grep -E '^api/examples/.*\.(json|ya?ml)$')
  for f in api/jsonschema/*.json; do
    for status in valid invalid; do
      [[ " ${covered[*]} " == *" ${status}:${f} "* ]] ||
        { printf 'spec: %s has no %s example that checks\n' "${f}" "${status}" >&2; bad=1; }
    done
  done
  [[ "${bad}" -eq 0 ]] || failed "spec (schemas)"
}

# buf format, lint and breaking (against main); the committed Go code is what buf generates.
spec_proto() {
  local base="${RANGELIGHT_SPEC_BASE:-origin/main}" tmp toolchain
  toolchain="$(go_toolchain)"
  (cd api && buf format --diff --exit-code && buf lint) || failed "spec (buf format or lint)"
  if git cat-file -e "${base}:api/buf.yaml" 2>/dev/null; then
    (cd api && buf breaking --against "${ROOT}/.git#ref=${base},subdir=api") ||
      failed "spec (breaking change against ${base})"
  else
    printf 'spec: %s has no api/ yet, nothing to break\n' "${base}"
  fi
  tmp="$(mktemp -d /tmp/rl-check-spec.XXXXXX)"
  # shellcheck disable=SC2064 # expand now: tmp is local
  trap "rm -rf '${tmp}'" EXIT
  cp -R api "${tmp}/api"
  cp go.mod go.sum "${tmp}/"
  (cd "${tmp}/api" && GOTOOLCHAIN="${toolchain}" buf generate) || failed "spec (buf generate)"
  diff -r "${tmp}/${SPEC_GO_GEN}" "${SPEC_GO_GEN}" >&2 ||
    { printf 'spec: %s is stale; run buf generate in api/\n' "${SPEC_GO_GEN}" >&2; failed "spec (generated code)"; }
}

spec_versions() {
  need buf check-jsonschema go
  [[ "$(buf --version)" == "${BUF_VERSION}" ]] ||
    { printf 'spec: need buf %s, have %s\n' "${BUF_VERSION}" "$(buf --version)" >&2; failed "spec (tool versions)"; }
  [[ "$(check-jsonschema --version)" == *" ${CHECK_JSONSCHEMA_VERSION}" ]] ||
    { printf 'spec: need check-jsonschema %s\n' "${CHECK_JSONSCHEMA_VERSION}" >&2; failed "spec (tool versions)"; }
}

spec() {
  spec_files
  spec_copies
  spec_versions
  spec_schemas
  spec_proto
  ok spec
}

# --- yaml: one YAML style (ADR 0049, amendment "layout and style from day one") ------

yaml() {
  local files=() bad=0 f
  need yamllint
  # Never empty: .yamllint.yaml itself is YAML.
  mapfile -t files < <(repo_files | grep -E '\.ya?ml$')
  [[ "$(yamllint --version)" == "yamllint ${YAMLLINT_VERSION}" ]] ||
    { printf 'yaml: need yamllint %s, have %s\n' "${YAMLLINT_VERSION}" "$(yamllint --version)" >&2; failed "yaml (tool version)"; }
  for f in "${files[@]}"; do
    [[ "${f}" == *.yml ]] || continue
    printf '%s: rename to .yaml\n' "${f}"
    bad=1
  done
  yamllint --strict --config-file .yamllint.yaml -- "${files[@]}" || bad=1
  [[ "${bad}" -eq 0 ]] || failed yaml
  ok yaml
}

# --- per-component parts --------------------------------------------------------

rust() {
  has_files '\.rs$' || { skip rust; return 0; }
  need cargo cargo-nextest cargo-deny cargo-machete
  cargo fmt --all --check
  cargo clippy --workspace --all-targets --locked -- -D warnings
  cargo nextest run --workspace --locked --no-tests=pass
  cargo deny check
  cargo machete
  ok rust
}

go() {
  has_files '\.go$' || { skip go; return 0; }
  need go
  local -x GOTOOLCHAIN
  GOTOOLCHAIN="$(go_toolchain)"
  command go run "${GOLANGCI_LINT}" run ./...
  command go test -race ./...
  command go run "${GOVULNCHECK}" ./...
  ok go
}

ts() {
  has_files '^services/(agent|web)/.*\.tsx?$' || { skip ts; return 0; }
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
  mapfile -t scripts < <(repo_files | grep -E '\.sh$|^services/git-mirror/hooks/[^/]+$')
  shellcheck -x -- "${scripts[@]}"
  if has_files "^${CHART}/Chart\.yaml$"; then
    bash "${CHART}/test/lint.sh"
    bash "${CHART}/test/template.sh"
  fi
  for t in scripts/test/*.sh; do bash "${t}"; done
  ok deploy
}

hooks() {
  has_files '^services/git-mirror/' || { skip hooks; return 0; }
  local tests=() t
  mapfile -t tests < <(repo_files | grep -E '^services/git-mirror/test/[^/]+\.sh$')
  [[ ${#tests[@]} -gt 0 ]] || { printf 'services/git-mirror/ has no tests in services/git-mirror/test/\n' >&2; failed hooks; }
  for t in "${tests[@]}"; do bash "${t}"; done
  ok hooks
}

# Rewrites files in place; not part of `all` (each part checks formatting itself).
fmt() {
  local did=0
  if has_files '\.rs$'; then cargo fmt --all; did=1; fi
  if has_files '\.go$'; then GOTOOLCHAIN="$(go_toolchain)" command go run "${GOLANGCI_LINT}" fmt; did=1; fi
  if has_files '^services/(agent|web)/.*\.tsx?$'; then pnpm exec prettier --write .; did=1; fi
  [[ "${did}" -eq 1 ]] || { skip fmt; return 0; }
  ok fmt
}

all() {
  local part bad=()
  for part in size rules secrets scrub spec yaml rust go ts deploy hooks; do
    bash "${ROOT}/scripts/check.sh" "${part}" || bad+=("${part}")
  done
  [[ ${#bad[@]} -eq 0 ]] || failed "all (${bad[*]})"
  ok all
}

# --- dispatcher -----------------------------------------------------------------

usage() {
  printf 'usage: scripts/check.sh <part>\nparts: %s\n' "${PARTS[*]}" >&2
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
