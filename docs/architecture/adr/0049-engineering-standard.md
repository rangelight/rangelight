# 0049. Engineering standard: enforced by CI, designed for extension at known seams

- Status: accepted
- Date: 2026-11-06
- Deciders: the owner
- Supersedes: ADR 0036 (engineering conventions). Its GitHub Actions pipeline and "why" comments in security code carry over into §8 and §9 below.
- Amends: ADR 0026 (monorepo layout), ADR 0035 (build layout)

## Context

The public repo will carry the owner's name. ADR 0036 adopted another project's conventions wholesale, and that project grew 3,000-line files and SQL inside Rust strings. This ADR replaces it with a standard written for Rangelight, where **every rule is checked by CI or by review against a named checklist**. If CI can't check a rule, the rule says how review checks it.

The standard has two jobs:
1. Keep the code small, typed and readable from the first commit.
2. Put stable seams exactly where the roadmap already says a second implementation is coming, and nowhere else, so growth doesn't force a rewrite.

## 1. Repository layout

```
spec/                   protobuf + JSON Schemas (Buf); the only source of cross-language types
control-plane/          Rust workspace
  Cargo.toml            [workspace] members, [workspace.dependencies], [workspace.lints]
  crates/<rl-name>/     libraries
  services/<name>/      binaries: api, relay, gateway, rld (later: checkpoint)
  queries/<area>/*.sql  every SQL statement
  migrations/           sqlx migrations, forward-only
runner/                 Go module (the Kubernetes runner)
agent/                  TypeScript (worker and dispatcher entrypoints)
web/                    TypeScript (the Workbench)
git-mirror/             nginx config and hook scripts
charts/rangelight/        the Helm chart
build/package/docker/<service>/Dockerfile
deploy/dev/             dev cluster scripts (k3d and any single-node cluster)
scripts/                repo checks (size, rules, scrub) and their self-tests
poc/                    POC checks and results (Plan 1); removed or folded in after v0.1
config/                 default.toml per service (no secrets)
docs/                   PRODUCT.md, architecture/ (ADRs in architecture/adr/), engineering/
justfile                the single entry point; `just check` is exactly what CI runs
```

**Organise by feature inside each component, not by technical layer.** `runner/internal/session/` holds the session's types, reconciler and tests together. There is no `models/`, `utils/` or `helpers/` folder anywhere.

## 2. Architecture rules (all languages)

### 2.1 Dependencies point inward

```
binaries (services/*, cmd/*, entrypoints)  →  adapters  →  application  →  domain
```

- **Domain code** holds types, state machines and rules. It does no I/O, reads no clock and no environment, and isn't async in Rust. It's tested without a network, a database or a cluster.
- **Application code** (use cases: start a lane, verify a branch, sign a receipt) is async, orchestrates the domain, and **declares the seam traits it needs**. In Rust this is `rl-app`; in Go and TS it's the feature package that consumes the interface.
- **Adapters** talk to the world: Postgres, NATS, Kubernetes, GitHub, Linear, Anthropic, OpenBao.
- **Binaries** only load config, start telemetry, wire adapters into the domain and run. A `main.rs`, `main.go` or `main.ts` stays under **80 lines**.
- **Enforced by:** the crate graph in Rust (`cargo-deny` bans adapter crates as dependencies of `rl-domain`), `depguard` in Go, and `eslint-plugin-boundaries` in TypeScript.

### 2.2 Seams: interfaces only where a second implementation is known

A seam is a Rust trait, a Go interface or a TS interface that the domain depends on, with adapters implementing it. **A seam is allowed only when one of these holds:**
- a second real implementation exists, or
- the roadmap (`ARCHITECTURE.md` §9 or §10) schedules one, or
- it replaces I/O that tests must make deterministic (clock, IDs, randomness).

Otherwise the code uses a concrete type. Adding a seam later is a small, mechanical change. Removing a wrong abstraction isn't.

**The seams known today:**

| Seam | Language | Now | Next (scheduled) | Owning plan |
|---|---|---|---|---|
| `AgentRuntime` | TS | Agent SDK session (`sdk`) | unmodified Claude Code in a PTY (`cli`, ADR 0046); other agents (target) | 1 (`sdk`), 5 (`cli`) |
| `TicketSource` | Rust | Linear | GitHub Issues (v0.2) | 4 |
| `Forge` (repo, PR, Check Run, App webhooks) | Rust | GitHub | GitLab, Bitbucket (target) | 4 |
| `Signer` | Rust | OpenBao Transit | BYO KMS (ADR 0041) | 3 |
| `Clock`, `IdGen` | all | system | fixed in tests | 1 (TS), 2 (Rust) |

**Not seams** (concrete on purpose): Postgres (`rl-store` is the only data access; no repository-trait-per-table layer), NATS (one transport, ADR 0021), Kubernetes (only the runner talks to it), the Anthropic upstream in the gateway (tests use a mock HTTP server, not a trait), object storage (one S3 client; RustFS and BYO S3 differ only in config), Cedar (tests use real Cedar with test policies; a fake would test less).

**Seam shape:**
- Small: the methods the domain actually calls, nothing speculative.
- Defined next to the code that *consumes* it: in Rust in `rl-app` or the consuming crate (`Signer` in `rl-receipt`), never in `rl-domain`, which stays sync and I/O-free; in Go in the consuming package; in TS in the consuming feature folder.
- Errors are the seam's own enum, never the adapter's library error.
- Each seam has a **contract test suite** (`rl-testkit` in Rust, a shared `describe` in TS) that every implementation, including the fake, must pass. A second implementation is "done" when it passes the same suite.

### 2.3 Contracts are the public interfaces

- **Cross-process types come only from `spec/`.** Rust, Go and TS code generated by Buf is never edited by hand and never re-declared.
- **Versioning (ADR 0027):** `v1alpha1` may break with a changelog entry. From `v1beta1` on, changes are additive only: no removed or renumbered fields, no changed meaning. `buf breaking` and the JSON Schema diff check run on every PR.
- **Every public contract has examples**, valid and invalid, that CI validates.

### 2.4 State and data

- **Postgres is the truth.** State + event + outbox are written in one transaction (ADR 0031).
- **SQL lives in files:** `control-plane/queries/<area>/<name>.sql`, loaded with `sqlx::query_file_as!`, compile-time checked. The `.sqlx/` offline data is committed and CI checks it's current (`cargo sqlx prepare --check`). **No SQL in Rust string literals**; a CI grep fails on `query!(`, `query_as!(` and `query(` with a literal.
- **Migrations** use `sqlx migrate`, forward-only, and follow **expand/contract**: no destructive change in the same release that stops using a column. A migration test applies every migration to an empty database and to the previous release's schema with data.
- **RLS and grants** have SQL check suites run against real Postgres (ADR 0030).

## 3. Rust

- **Toolchain:** pinned in `rust-toolchain.toml` (stable, with `rustfmt`, `clippy`). Edition 2024.
- **Workspace lints** (`[workspace.lints]`, inherited by every crate):
  - `rust`: `unsafe_code = "forbid"`, `missing_docs = "warn"` (deny in library crates), `unreachable_pub = "warn"`
  - `clippy`: `pedantic = "warn"`, and `unwrap_used`, `expect_used`, `panic`, `todo`, `unimplemented`, `dbg_macro`, `print_stdout`, `print_stderr` = `"deny"`
  - Tests may `unwrap` (`#[cfg(test)]` allow at module level only).
  - CI runs `cargo clippy --workspace --all-targets -- -D warnings`.
- **Dependencies:** declared once in `[workspace.dependencies]`; crates use `dep.workspace = true`. `cargo-deny` checks licences (Apache-2.0 compatible), advisories, bans and duplicate versions. `cargo-machete` finds unused ones.
- **Crates** (created when first needed, not up front):

  | Crate | Holds | May depend on |
  |---|---|---|
  | `rl-domain` | IDs, lane state machine, contract and criteria rules (sync, no I/O) | std, `thiserror`, `serde` |
  | `rl-app` | use cases and the `TicketSource`, `Forge`, `Clock`, `IdGen` seam traits (async) | `rl-domain`, `async-trait` if needed |
  | `rl-config` | the loader (defaults → file → env), secret references, validation helpers; each service declares its own `Config` struct in its `config.rs` | `figment`, `serde`, `secrecy` |
  | `rl-telemetry` | `tracing` setup, OTel export, metrics | `tracing*`, `opentelemetry*` |
  | `rl-proto` | Buf-generated code | generated only |
  | `rl-store` | sqlx access, one module per area, mirroring `queries/` | `rl-domain`, `sqlx` |
  | `rl-chain` | event canonicalisation, hashing, chain verify | `rl-domain`, `rl-proto` |
  | `rl-receipt` | the in-toto predicate, DSSE, the `Signer` seam | `rl-domain`, `rl-chain` |
  | `rl-policy` | Cedar policies, entities and evaluation | `rl-domain`, `cedar-policy` |
  | `rl-forge-github`, `rl-tickets-linear` | adapters for their seams | `rl-app`, HTTP client |
  | `rl-testkit` | fakes, contract suites, Postgres fixtures (dev-dependency only) | anything |

  **A new crate needs a reason:** reuse by two binaries, a trust boundary, or a seam adapter. Never one crate per feature.
- **Binaries** (`services/<name>`) have a `lib.rs` with the wiring and modules, and a `main.rs` under 80 lines. Integration tests use the lib.
- **Errors:** `thiserror` enums in libraries, one per crate or per seam, with variants a caller can act on. `anyhow` only in `main.rs`. At the API edge, errors map to stable Connect codes plus a machine-readable reason; internal details go to the log, never to the client.
- **Async:** Tokio. No blocking calls on the runtime (`spawn_blocking` for CPU or sync I/O). Every outbound call has a timeout. Cancellation is by `CancellationToken`, passed down.
- **Observability:** `tracing` only. One span per request and per session operation, with `session_id`, `org_id` and `trace_id` fields. Secrets are wrapped in `secrecy::SecretString` and never implement `Debug` or `Display`.
- **Config:** see §6.

## 4. Go (`runner/`)

- Go version pinned in `go.mod` and `toolchain`.
- `golangci-lint` with a committed `.golangci.yml`: `govet`, `staticcheck`, `errcheck`, `errorlint`, `wrapcheck`, `gosec`, `revive`, `gocognit` (15), `funlen` (60 lines), `depguard`, `forbidigo` (`fmt.Print*`, `os.Getenv` outside `internal/config`), `exhaustive`.
- Errors wrapped with `%w`; sentinel errors or typed errors at package boundaries. No `panic` outside `main`.
- Logging with `log/slog` (JSON), the same field names as Rust.
- Interfaces are declared in the consuming package and kept small (§2.2).
- Tests: table-driven, `envtest` for reconcilers, `-race` in CI, `govulncheck`.

## 5. TypeScript (`agent/`, `web/`)

- Node LTS pinned in `.nvmrc` and `engines`; pnpm workspace with a committed lockfile.
- `tsconfig`: `strict`, `noUncheckedIndexedAccess`, `exactOptionalPropertyTypes`, `noImplicitOverride`, `verbatimModuleSyntax`.
- ESLint (flat config) with `typescript-eslint` `strict-type-checked`, `eslint-plugin-boundaries`, `no-console` (except the agent's event writer), and `max-lines` / `max-lines-per-function` matching §7. Prettier for formatting.
- **No `any`**, no non-null assertions (`!`), no `as` casts except at a validated boundary. External data (env, files, network, SDK messages) is parsed with zod at the edge; inside, types are trusted.
- Errors: typed result unions for expected failures; thrown `Error` subclasses for bugs only.
- Tests: vitest; Playwright for the Workbench.

### 5.1 Web UI styling (`web/`)

- **Colours, spacing, radii and type come only from the design tokens** in `web/src/styles/tokens.css` (seeded from the mock-up's `index.css`, where `--color-lf` is renamed `--color-rl`: `--color-rl`, `--color-ok`, `--color-warn`, `--color-bad`, `--color-t2`, `--color-dim`, `--color-faint`, `--color-line`, `--color-line-2`, surfaces, and their dark/light values). A lint rule fails on raw hex, `rgb(`, or arbitrary Tailwind colour values (`text-[#…]`) outside `tokens.css`.
- **Components:** primitives in `web/src/components/ui` (shadcn-derived), product components in `web/src/components/lf`. Views compose them; a view doesn't define its own button, chip or panel.
- **Feature folders:** `web/src/features/<feature>/` holds its views, hooks, state and tests. Shared state goes through one store module per feature.
- **Both themes and reduced motion** are tested: Playwright screenshots in light and dark for each view, and `prefers-reduced-motion` respected by every animation.
- **Accessibility:** every interactive element is keyboard-reachable with a visible focus ring; `axe` runs in Playwright with zero serious violations.

## 6. Configuration

- **One typed config per service**, loaded in this order: built-in defaults → `config/<service>.default.toml` → an optional mounted file → environment variables prefixed `RANGELIGHT_<SERVICE>_`. Validated at startup; the service exits with a clear message on any invalid value.
- **Only the config module reads the environment** (`rl-config`, `runner/internal/config`, `agent/src/config.ts`). A CI grep enforces it.
- **Exception:** variables a third-party library reads itself (for example `ANTHROPIC_BASE_URL` for the Agent SDK) keep their names. The config module lists them in one place and sets them for the library, so they're still visible in one file.
- **Secrets are references** (`secret://<name>#<key>`, or an OpenBao path from Plan 3), never values in config. Until OpenBao, `secret://<name>#<key>` resolves to the mounted Secret file `/var/run/rangelight/secrets/<name>/<key>`; services never call the Kubernetes API for secrets. The value is read at use, held in a secret type, and never logged.
- **Constants that never change aren't config** (YAGNI). The API group and the predicate type URI are single constants per language (values in ADR 0051).

## 7. Size and shape, failed by CI

Counted files: `.rs`, `.go`, `.ts`, `.tsx`, `.sh`, `.sql`. Not counted: generated code (`gen/`, `rl-proto`, `.sqlx/`), JSON, YAML, Markdown, and test files (`tests/`, `*_test.go`, `*.test.ts`, Rust `tests.rs` files). **Rust unit tests go in a sibling `tests.rs`** (`#[cfg(test)] mod tests;`), so inline test code never pushes a source file over the limit.

| Rule | Limit | Check |
|---|---|---|
| Source file length | 400 lines | `scripts/check-size.sh` in `just check` |
| Function length | 60 lines | clippy `too_many_lines`, `funlen`, ESLint `max-lines-per-function` |
| Cognitive complexity | 15 | clippy `cognitive_complexity`, `gocognit`, `sonarjs/cognitive-complexity` |
| Binary entrypoint | 80 lines | `check-size.sh` |
| Function parameters | 5 | clippy `too_many_arguments`, ESLint `max-params` |

An exception needs a `// lint: <rule> because <reason>` comment on the line and a reviewer's approval; `check-size.sh` keeps a list of excepted files in the repo, which must stay short.

## 8. Tests

- **Every task ships its tests in the same PR.** Every non-trivial branch, loop, parser, money path or security path has a test that fails if it breaks.
- **Unit tests** live next to the code. **Integration tests** use real dependencies: `sqlx::test` against real Postgres, a real NATS server, envtest, a real git binary. We don't mock our own database or transport.
- **Property tests** (`proptest`) for the chain, receipts and canonical encoding. **Fuzzing** (`cargo-fuzz`, nightly toolchain) for every parser that takes untrusted input (SSE, JWTs, webhook bodies, MCP framing), in a separate nightly CI job (`just fuzz`), not in `just check`. **Snapshot tests** (`insta`) for API responses and receipts.
- **Security tests are named after the threat-model item** they close (`t16_agent_cannot_reach_apiserver`), so `grep t<number>_` shows the coverage of `threat-model.md` §7.
- **Live tests** (real Anthropic, real GitHub) are tagged and run on demand, never in the default CI run.
- Coverage isn't a target. Untested security or money code is a review blocker.

## 9. CI, commits and review

- **`just check`** is the whole local gate and exactly what CI runs: format, lint, size, tests per language, `buf lint` + `buf breaking`, schema checks, `cargo-deny`, `cargo-machete`, `govulncheck`, `pnpm audit --prod`, `cargo sqlx prepare --check`, the SQL check suites and the hook tests. **The GitHub Actions pipeline from ADR 0036** (fuzz smoke, receipt-verify fixtures, k3d timing check, cosign, SLSA) runs on top as its plans add them.
- **Commits:** Conventional Commits, DCO sign-off (`git commit -s`), signed commits. One logical change per commit.
- **PRs:** small (aim under 400 changed lines excluding generated code), one concern, with a template that links the ADR or ticket, lists the tests added, and ticks the threat-model items touched. CODEOWNERS on `spec/`, `git-mirror/hooks/`, `control-plane/services/gateway/`, `control-plane/services/rld/`, `rl-receipt`, `rl-chain`, `rl-policy` and `migrations/`.
- **Branch protection on `main`:** required CI, one review, linear history, no force-push, no bypass. How work flows around this (trunk, releases, agent identity, tickets) is ADR 0050.
- **`just check` also runs** `gitleaks`, and `check-deploy` (`helm lint`, `helm template | kubeconform`, `shellcheck`) (ADR 0050 §7–§8).

## 10. Comments and documentation

- **Doc comments on every public item** in library crates and exported Go/TS symbols: what it does and its contract (errors, panics: none). Not how.
- **Inline comments explain why**, only where the code can't: a security reason, a spec or RFC requirement, a workaround with a link. Security-critical code (gateway, hooks, chain, receipt, RLS, policy) may carry more of them, since outside reviewers won't have the tickets (from ADR 0036).
- **No commented-out code, no `TODO` without a ticket ID** (`TODO(RAN-123): …`, checked by CI).
- **Each component has a `README.md`** of one screen: what it is, how to run its tests, where its contract lives.
- **Shortcuts with a known ceiling** are marked `ponytail:` with the ceiling and the upgrade path, and are listed in the plan report.

## 11. Agents follow the same standard

- The repo's `CLAUDE.md` is a short pointer to this ADR plus the commands (`just check`, `just dev-up`), not a second copy of the rules.
- Agent-written code goes through the same CI and the same review. "An agent wrote it" is never a reason for an exception.

## Consequences

- The first commit of the new monorepo is the standard itself: workspace lints, `justfile`, `check-size.sh`, linters and CI (Plan 1, Task 1). Every later task inherits it.
- The mock-up's `Repos.tsx` (509 lines) and `chapters.ts` (486 lines) must be split before they move into `web/` (Plan 5).
- Adding a seam is a reviewed decision: the PR names the second implementation or the roadmap line that schedules it.
- Some early code is a little longer than the shortest possible version (typed config, error enums, doc comments). That's the price of a codebase strangers can read.

## Revisit if

- A limit in §7 blocks clear code more than occasionally. Then raise the limit in this ADR rather than adding exceptions.
- A third implementation of a seam needs a capability the trait can't express. Then version the trait, don't widen it ad hoc.

## Amendment (2026-11-06, groundwork research)

- **Names from before the rename are gone before commit 1** (ADR 0051 freezes names at the first public commit): crates are `rl-*` (`rl-domain`, `rl-app`, …), the sidecar is `rld`, NATS subjects start `rl.`, Postgres roles are `rl_<role>`, session namespaces `rl-<session>`, the accent token `--color-rl`, and ticket keys `RAN-<n>` (`TODO(RAN-123): …`; ADR 0051 amendment).
- **§8, fuzzing is detectable:** Go native fuzz tests (`func FuzzX(f *testing.F)`) in `runner/` for every parser of untrusted input, and the Rust `cargo-fuzz` targets run through ClusterFuzzLite in the nightly job. OpenSSF Scorecard's Fuzzing check doesn't see `cargo-fuzz` alone.
- **§11:** `AGENTS.md` is the canonical agent pointer; `CLAUDE.md` is a one-line alias to it. Both stay short (commands plus a link to this ADR).

## Amendment (2026-11-07, no task runner)

- **Developer commands are Bash files in `script/`**, one per area (`check.sh`, `cluster.sh`, `demo.sh`, `fuzz.sh`, `metrics.sh`). Each file is `#!/usr/bin/env bash` and `set -euo pipefail`, takes paths from `git rev-parse --show-toplevel`, has one function per command, and ends with a `declare -f` dispatcher that prints the usage and exits 2 on an unknown name. Variables are quoted; `shellcheck` passes.
- **CI runs `script/check.sh <part>`**, exactly as a developer does. This replaces "the `justfile` is the single entry point" in §1, and every `just check` in this ADR now reads `script/check.sh all`.
- **No `just`, `make` or `task`.** If a runner is ever added, it only calls these files and holds no logic.
- Files baked into images (for example `git-mirror/hooks/*`, `git-mirror/sync.sh`) and one-off POC checks (`poc/checks/*`) stay with their component.
- Owner's decision, 2026-11-07: task-runner files must not become bloated; plain Bash functions with clear syntax are the entry point, and a runner may at most wrap them.

## Amendment (2026-11-07, layout and style from day one)

Owner decision. Rangelight follows [golang-standards/project-layout](https://github.com/golang-standards/project-layout) for its cross-cutting directories, and one YAML style everywhere.

- **Layout changes to §1** (this replaces §1's component directories):
  - **`services/<name>/` holds every deployable, whatever its language**: `api`, `relay`, `gateway` and `rld` (Rust), `runner` (Go), `agent` and `web` (TypeScript), and `git-mirror`. A service's private code lives inside it, for example `services/runner/internal/<feature>/`.
  - **`shared/<lang>/<name>/`** holds libraries: `shared/rust/<name>` (packages keep their `rl-<name>` names), `shared/go/<name>`, and `shared/ts/<name>` when needed.
  - There's one Rust workspace (a root `Cargo.toml`), one Go module (a root `go.mod`, `github.com/rangelight/rangelight`), and one pnpm workspace.
  - `spec/` becomes `api/`. SQL moves to `db/migrations/` and `db/queries/<area>/`.
  - `deploy/dev/` becomes `deployments/dev/`, and `charts/rangelight/` becomes `deployments/helm/rangelight/`.
  - `config/` becomes `configs/`.
  - Developer commands live in **`scripts/`** (plural; this corrects the earlier amendment's `script/`).
  - CI helper scripts live in `build/ci/`. Workflows stay in `.github/workflows/`, where GitHub requires them.
  - End-to-end tests and shared test data live in `test/`. Unit tests stay next to the code.
  - The top-level `control-plane/`, `runner/` and `agent/` directories are gone. There's no `cmd/` (`services/<name>` is the binary) and no `pkg/` (Rangelight publishes no Go library API).
- **Standalone:** Rangelight never assumes a cluster add-on such as cert-manager, a Postgres operator, Argo CD or a storage driver. It brings what it needs (ADR 0009) or detects it by behaviour (the preflight).
- **YAML:**
  - Every file starts with `---` and ends in `.yaml`, never `.yml`.
  - String values are double-quoted, including list items and flow lists. Keys stay bare, and so do booleans and numbers, unless the field's type is a string.
  - Kubernetes objects carry `app.kubernetes.io/name`, `component`, `part-of: "rangelight"` and `managed-by`.
  - Enforced by `yamllint` in `script/check.sh yaml`.
- **How code is written ("ponytail"):** stop at the first rung that holds:
  1. Does it need to exist at all?
  2. Is it already in this repo?
  3. Does the standard library do it?
  4. Does a native platform feature cover it?
  5. Does an existing dependency do it?
  6. Can it be one line?
  7. Only then, write the minimum code that works.

  No abstraction with one implementation, no scaffolding "for later", no config for a constant. Fewest files; shortest working diff. Never cut validation at trust boundaries, error handling that prevents data loss, or security. Every non-trivial branch leaves one small test.
