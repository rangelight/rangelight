# Rangelight

Range lights are a pair of shore lights: a pilot keeps the front and rear light in line to stay in the
channel. Rangelight lands an agent's change only when two independent signals line up: the agent's
claim, and Rangelight's own check of it ([ADR 0051](docs/architecture/adr/0051-name-rangelight.md)).

The project is at its first commit: this tree holds the engineering standard, the checks and CI. The
components arrive with their tickets.

## Layout

| Path | What |
|---|---|
| `spec/` | Protobuf and JSON Schemas (Buf), the only source of cross-language types |
| `control-plane/` | Rust workspace: `crates/rl-*` libraries, `services/*` binaries, `queries/`, `migrations/` |
| `runner/` | Go module, the Kubernetes runner |
| `agent/`, `web/` | TypeScript: the agent entrypoints and the Workbench |
| `charts/rangelight/` | The Helm chart |
| `build/package/docker/<service>/` | One Dockerfile per image |
| `script/` | Developer commands (Bash) and their self-tests |
| `docs/` | Architecture decisions (`docs/architecture/adr/`) and engineering notes |

The full layout and its rules are [ADR 0049](docs/architecture/adr/0049-engineering-standard.md).

## Commands

```sh
script/check.sh all        # the whole local gate; CI runs the same parts
script/check.sh <part>     # one part: size rules secrets scrub spec rust go ts deploy hooks
script/check.sh fmt        # format every component in place
script/test/check.sh       # self-tests of the checks
```

There is no `make`, `just` or `task`: every command is a function in a Bash file in `script/`.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). Contributions are under the DCO; there is no CLA.

## Licence

Apache-2.0, see [LICENSE](LICENSE). The code in this repository is never relicensed
([ADR 0052](docs/architecture/adr/0052-open-core-boundary.md)).
