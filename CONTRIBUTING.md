# Contributing to Rangelight

Thanks for helping. The engineering standard is
[ADR 0049](docs/architecture/adr/0049-engineering-standard.md) and the delivery practices are
[ADR 0050](docs/architecture/adr/0050-delivery-practices.md). This page is the short version.

## Before you open a pull request

1. **Run the gate.** `script/check.sh all` is exactly what CI runs. It must exit 0.
   One part at a time: `script/check.sh <part>` (run `script/check.sh` to list them).
2. **Ship the tests with the change.** Every branch, parser, money path or security path has a test
   that fails if it breaks. Acceptance tests are named after the ticket's criteria (`AC1`, `AC2`, ...).
3. **Keep it small.** One concern per pull request, aiming under 400 changed lines excluding
   generated code. Files stay under 400 lines and functions under 60 (ADR 0049 §7).

## Commits

- **Conventional Commits:** `feat: ...`, `fix: ...`, `docs: ...`, `chore: ...`, `refactor: ...`,
  `test: ...`, `ci: ...`, with an optional scope (`feat(runner): ...`). The changelog is generated
  from them.
- **DCO sign-off on every commit:** `git commit -s`. It adds
  `Signed-off-by: Your Name <you@example.com>`, certifying the
  [Developer Certificate of Origin](https://developercertificate.org/). There is no CLA: you keep
  your copyright.
- **Signed commits.** `main` accepts signed commits only.
- One logical change per commit.

## Branches

Every branch except `main` is named `<type>/RAN-<n>-<word>-<word>`, where the type is `feat`,
`bug`, `hotfix` or `release` and `RAN-<n>` is the ticket, in upper case. For example
`feat/RAN-42-receipt-verify`. CI rejects any other name.

## Code markers

- `TODO` always carries a ticket: `TODO(RAN-123): ...`. CI fails a bare one.
- A rule exception carries `// lint: <rule> because <reason>` on the line and needs a reviewer's
  approval.

## AI-assisted contributions

Welcome, with disclosure: say so in the pull request, link the accepted issue, and sign off as the
accountable human (ADR 0052 §8).

## Security issues

Don't open a public issue for a vulnerability. Contact the maintainers privately through GitHub's
private vulnerability reporting on this repository.
