# shellcheck shell=bash
# The per-language parts: yaml, rust, go, ts, deploy, hooks, and fmt. Sourced by scripts/check.sh, which holds the helpers, the pins and the dispatcher.

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
  need cargo cargo-nextest cargo-deny cargo-machete cargo-sqlx buf # buf: rl-proto's build script
  local -x SQLX_OFFLINE=true # SQL compiles against the committed .sqlx/ (ADR 0049 §2.4)
  cargo fmt --all --check
  cargo clippy --workspace --all-targets --locked -- -D warnings
  bash scripts/test/services.sh run bash -c 'cargo sqlx migrate run --source db/migrations &&
    cargo sqlx prepare --workspace --check -- --all-targets --locked && cargo nextest run --workspace --locked --no-tests=pass'
  cargo deny check
  cargo machete
  ok rust
}

go() {
  has_files '\.go$' || { skip go; return 0; }
  need go helm
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
    for t in template git-mirror runner; do bash "${CHART}/test/${t}.sh"; done
  fi
  for t in scripts/test/*.sh; do [[ ! -f "scripts/${t##*/}" ]] || bash "${t}"; done # self-tests; services.sh is a fixture
  ok deploy
}

hooks() {
  has_files '^services/git-mirror/' || { skip hooks; return 0; }
  local tests=() t
  mapfile -t tests < <(repo_files | grep -E '^services/git-mirror/.*_test\.sh$')
  [[ ${#tests[@]} -gt 0 ]] || { printf 'services/git-mirror/ has no *_test.sh\n' >&2; failed hooks; }
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
