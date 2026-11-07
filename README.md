# Rangelight

Range lights are a pair of shore lights: a pilot keeps the front and rear light in line to stay in the
channel. Rangelight lands an agent's change only when two independent signals line up: the agent's
claim, and Rangelight's own check of it ([ADR 0051](docs/architecture/adr/0051-name-rangelight.md)).

The project is at its first commit: this tree holds the engineering standard, the checks and CI. The
components arrive with their tickets.

## Layout

| Path | What |
|---|---|
| `services/<name>/` | Every deployable, any language: `api`, `relay`, `gateway`, `rld` (Rust), `runner` (Go), `agent`, `web` (TypeScript), `git-mirror` |
| `shared/<lang>/<name>/` | Libraries: `shared/rust/*` (packages `rl-<name>`), `shared/go/*` (generated protobuf in `shared/go/gen/`), `shared/ts/*` |
| `api/` | Protobuf and JSON Schemas (Buf), the only source of cross-language types |
| `db/` | SQL: `migrations/` and `queries/<area>/` |
| `deployments/` | `dev/` (the dev environment) and `helm/rangelight/` (the Helm chart) |
| `build/` | `package/docker/<service>/Dockerfile` (one per image) and `ci/` helpers |
| `scripts/` | Developer commands (Bash) and their self-tests |
| `docs/` | Architecture decisions (`docs/architecture/adr/`) and engineering notes |

The root holds the one Rust workspace (`Cargo.toml`), the one Go module (`go.mod`) and the one pnpm
workspace (`pnpm-workspace.yaml`).

The full layout and its rules are [ADR 0049](docs/architecture/adr/0049-engineering-standard.md)
(see its 2026-11-07 amendment).

## Commands

```sh
scripts/check.sh all        # the whole local gate; CI runs the same parts
scripts/check.sh <part>     # one part: size rules secrets scrub spec yaml rust go ts deploy hooks
scripts/check.sh fmt        # format every component in place
scripts/test/check.sh       # self-tests of the checks
```

There is no `make`, `just` or `task`: every command is a function in a Bash file in `scripts/`.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). Contributions are under the DCO; there is no CLA.

## Licence

Apache-2.0, see [LICENSE](LICENSE). The code in this repository is never relicensed
([ADR 0052](docs/architecture/adr/0052-open-core-boundary.md)).
