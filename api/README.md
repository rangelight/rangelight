# api

The contracts between Rangelight's parts and the world. They are the only source of cross-language
types: code generated from them is never edited by hand or re-declared
([ADR 0049 §2.3](../docs/architecture/adr/0049-engineering-standard.md)).

| Contract | File | Audience |
|---|---|---|
| Runner protocol: control plane ↔ runner, protobuf over NATS | `proto/rangelight/runner/v1alpha1/runner.proto` | public |
| Receipt predicate: `predicateType` `https://rangelight.dev/receipt/v1alpha1` | `jsonschema/receipt-predicate.v1alpha1.schema.json` | public |
| `rangelight.yaml`: the repo's preview profile, `apiVersion: rangelight.dev/v1alpha1` | `jsonschema/rangelight.v1alpha1.schema.json` | public |
| Agent event: the typed messages a lane may send (`ask`, `worker_done`, `progress`, `compacted`) | `jsonschema/agent-event.v1alpha1.schema.json` | internal, versioned |

The three public contracts are ADR 0025's launch set. The agent event is part of the session
protocol, which stays internal until a second agent implementation exists (ADR 0020, ADR 0025).
Schema `$id`s live under `https://rangelight.dev/schemas/` (ADR 0051).

`examples/` holds examples that validate, and `examples/invalid/` holds examples that must fail.
An example's first name segment picks its schema: `receipt.*`, `rangelight.*`, `agent-event.*`.

## Check and generate

```sh
script/check.sh spec    # from the repo root: everything below
cd api && buf generate # after a proto change; writes shared/go/gen/, which is committed
```

`script/check.sh spec` fails when a contract file is missing, a schema isn't valid against its
metaschema, an example doesn't validate or an invalid example does, a schema lacks a valid or an
invalid example, `buf format` or `buf lint` complains, `buf breaking` finds a break against `main`,
`shared/go/gen/` isn't what `buf generate` writes, or a schema copy in
`services/runner/internal/session/schema/`
differs from its source here. It needs buf 1.75.0, check-jsonschema 0.38.2 and Go; protoc-gen-go is
pinned by the root `go.mod`.

## Runner protocol subjects

`<org>` is the org's NATS account, `<runner>` the runner id, `<session>` the session UUIDv7
(ADR 0021). Every message is an `Envelope`; receivers dedupe on `message_id`.

| Subject | Direction | Kind | Message |
|---|---|---|---|
| `rl.<org>.runner.hello` | runner → CP | request/reply | `RunnerHello` → `RunnerWelcome` |
| `rl.<org>.runner.<runner>.desired` | CP → runner | JetStream, last value per session | `SessionDesired` |
| `rl.<org>.runner.<runner>.status` | runner → CP | JetStream, ordered per session | `SessionStatus` |
| `rl.<org>.runner.<runner>.capacity` | runner → CP | JetStream | `CapacityReport` |
| `rl.<org>.runner.<runner>.cmd` | CP → runner | request/reply | `Command` → `CommandResult` |
| `rl.<org>.runner.<runner>.gate` | runner → CP | JetStream, ordered per session | `GateResult` |

The control plane is the source of truth: the runner ignores a `SessionDesired.generation` older than
the one it has seen. `credential_refs` carries references, never secret values (ADR 0004). The profile
is read from the approved base branch, never the session branch (ADR 0041). Control plane N accepts
runners on protocol N-1 and N (ADR 0027).

## Versioning

`v1alpha1` may break, with an entry below. From `v1beta1` on, changes are additive only; `v1` is
frozen (ADR 0027). Receipt predicate versions are never removed, so receipts verify forever.

## Changelog

- **v1alpha1** (RAN-15): first version of all four contracts, from the design drafts. Runner protocol
  enum values carry their enum's prefix (`PHASE_RUNNING`, not `RUNNING`), as `buf lint` requires.
