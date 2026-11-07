# Agents

Follow [ADR 0049](docs/architecture/adr/0049-engineering-standard.md) (the engineering standard) and
[ADR 0050](docs/architecture/adr/0050-delivery-practices.md) (delivery). Agent-written code gets no
exceptions.

## Commands

```sh
scripts/check.sh all      # the whole gate, exactly what CI runs; must exit 0 before a PR
scripts/check.sh <part>   # size rules secrets scrub spec yaml rust go ts deploy hooks
scripts/check.sh fmt      # format in place
scripts/check.sh gen      # regenerate shared/ts/gen from api/jsonschema
scripts/test/check.sh     # self-tests of the checks
```

Branches: `<type>/RAN-<n>-<word>-<word>`. Commits: Conventional Commits, signed off (`-s`).

## How code is written here

Climb this ladder and stop at the first rung that holds:

1. Does it need to exist? If not, don't write it.
2. Does this repo already have it? Use that.
3. Does the standard library do it?
4. Does the platform do it natively (Kubernetes, Postgres, git, the shell)?
5. Does a dependency we already have do it?
6. Can it be one line?
7. Only then, write the least code that works.

No abstraction with a single implementation, no scaffolding for later, no config for a constant.
Fewest files, shortest working diff, short explanations. Every non-trivial branch gets one small test.

Never cut, however small the diff: validation at trust boundaries, error handling that prevents data
loss, and security.

**YAML:** extension `.yaml` (never `.yml`), first line `---`, 2-space indent. String values in double
quotes, list items and flow lists too (`image: "ghcr.io/x:1.2.3"`, `["get", "list"]`); keys bare;
booleans and numbers bare unless the field is a string. Kubernetes objects carry
`app.kubernetes.io/name`, `component`, `part-of: "rangelight"` and `managed-by`.
`scripts/check.sh yaml` enforces it.

**Layout** (ADR 0049, amendment 2026-11-07):

- `services/<name>/`: every deployable, any language; its private code lives inside it
  (`services/runner/internal/<feature>/`). No `cmd/`, no `pkg/`.
- `shared/rust/<name>/` (package `rl-<name>`), `shared/go/<name>/`, `shared/ts/<name>/`: libraries.
- Root `Cargo.toml`, `go.mod` and `pnpm-workspace.yaml`: one workspace per language.
- `api/` contracts, `db/migrations/` and `db/queries/<area>/` SQL, `deployments/dev/` and
  `deployments/helm/rangelight/`, `configs/`, `build/ci/`, `build/package/docker/<service>/`,
  `test/` (end-to-end), `scripts/` (developer commands), `docs/`, `fuzz/`.
- Never assume a cluster add-on (cert-manager, a Postgres operator, Argo CD, a storage driver):
  bring it or detect it.
