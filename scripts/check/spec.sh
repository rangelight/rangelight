# shellcheck shell=bash
# The contract parts: spec, and gen, which writes the TypeScript types. Sourced by scripts/check.sh, which holds the helpers, the pins and the dispatcher.

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
# The TypeScript types of the JSON Schemas (ADR 0049 §2.3); `gen` writes them there.
SPEC_TS_GEN="shared/ts/gen"

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

# ts_types <dir>: writes the TypeScript types of api/jsonschema/*.schema.json to <dir>, with the
# json-schema-to-typescript pinned in package.json.
ts_types() {
  pnpm exec json2ts --input 'api/jsonschema/*.schema.json' --output "$1" \
    --unreachableDefinitions --unknownAny --maxItems=-1 \
    --bannerComment '// Generated from api/jsonschema/ by scripts/check.sh gen. Do not edit.' >/dev/null
}

# The committed types are what the generator makes from the schemas.
spec_ts() {
  local tmp
  need pnpm
  tmp="$(mktemp -d /tmp/rl-check-spec-ts.XXXXXX)"
  # shellcheck disable=SC2064 # expand now: tmp is local
  trap "rm -rf '${tmp}'" EXIT
  ts_types "${tmp}/gen" || failed "spec (generate types)"
  diff -r "${tmp}/gen" "${SPEC_TS_GEN}" >&2 ||
    { printf 'spec: %s is stale; run scripts/check.sh gen\n' "${SPEC_TS_GEN}" >&2; failed "spec (generated types)"; }
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
  spec_ts
  spec_proto
  ok spec
}

# Rewrites shared/ts/gen from the schemas; not part of `all` (spec checks it is current).
gen() {
  need pnpm
  rm -rf "${SPEC_TS_GEN}"
  ts_types "${SPEC_TS_GEN}"
  ok gen
}
