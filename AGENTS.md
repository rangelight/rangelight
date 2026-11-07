# Agents

Follow [ADR 0049](docs/architecture/adr/0049-engineering-standard.md) (the engineering standard) and
[ADR 0050](docs/architecture/adr/0050-delivery-practices.md) (delivery). Agent-written code gets no
exceptions.

## Commands

```sh
script/check.sh all      # the whole gate, exactly what CI runs; must exit 0 before a PR
script/check.sh <part>   # size rules secrets scrub spec rust go ts deploy hooks
script/check.sh fmt      # format in place
script/test/check.sh     # self-tests of the checks
```

Branches: `<type>/RAN-<n>-<word>-<word>`. Commits: Conventional Commits, signed off (`-s`).
