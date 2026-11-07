# api

The contracts between Rangelight's parts and the world. They are the only source of cross-language
types: code generated from them is never edited by hand or re-declared
([ADR 0049 §2.3](../docs/architecture/adr/0049-engineering-standard.md)).

| Contract | File | Audience |
|---|---|---|
| Runner protocol: control plane ↔ runner, protobuf over NATS | `proto/rangelight/runner/v1alpha1/runner.proto` | public |
| Session API: Connect-RPC `SessionService`, and the session event payload | `proto/rangelight/session/v1alpha1/session.proto` | public |
| Sign-in: Connect-RPC `AuthService` (who is signed in) | `proto/rangelight/auth/v1alpha1/auth.proto` | public |
| Receipt predicate: `predicateType` `https://rangelight.dev/receipt/v1alpha1` | `jsonschema/receipt-predicate.v1alpha1.schema.json` | public |
| `rangelight.yaml`: the repo's preview profile, `apiVersion: rangelight.dev/v1alpha1` | `jsonschema/rangelight.v1alpha1.schema.json` | public |
| Agent event: the typed messages a lane may send (`ask`, `worker_done`, `progress`, `compacted`) | `jsonschema/agent-event.v1alpha1.schema.json` | internal, versioned |
| Ticket: the one ticket rule, and the lane's `/etc/rangelight/ticket.json` | `jsonschema/ticket.v1alpha1.schema.json` | internal, versioned |

The three public contracts are ADR 0025's launch set. The agent event is part of the session
protocol, which stays internal until a second agent implementation exists (ADR 0020, ADR 0025).
Schema `$id`s live under `https://rangelight.dev/schemas/` (ADR 0051).

`examples/` holds examples that validate, and `examples/invalid/` holds examples that must fail.
An example's first name segment picks its schema: `receipt.*`, `rangelight.*`, `agent-event.*`,
`ticket.*`.

**The ticket rule** lives only in `jsonschema/ticket.v1alpha1.schema.json`: the api's intake
(`rl-domain`, which tests its limits and these examples against the schema) skips a ticket that
breaks it, the Session CRD's `spec.ticket` is tested to be the same schema, and the lane reads a
`ticket.json` that already passed both. A session's ticket is frozen when the session is created:
later edits in the tracker don't reach its lane.

## Check and generate

```sh
scripts/check.sh spec    # from the repo root: everything below
cd api && buf generate # after a proto change; writes shared/go/gen/, which is committed
```

`scripts/check.sh spec` fails when a contract file is missing, a schema isn't valid against its
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
| `rl.<org>.runner.<runner>.desired.<session>` | CP → runner | JetStream, one message per subject | `SessionDesired` |
| `rl.<org>.runner.<runner>.status` | runner → CP | JetStream, ordered per session | `SessionStatus` |
| `rl.<org>.runner.<runner>.capacity` | runner → CP | JetStream | `CapacityReport` |
| `rl.<org>.runner.<runner>.cmd` | CP → runner | request/reply | `Command` → `CommandResult` |
| `rl.<org>.runner.<runner>.gate` | runner → CP | JetStream, ordered per session | `GateResult` |

The desired stream keeps one message per subject (`max_messages_per_subject` 1, no age limit), so
it is the desired-state table: each session's latest `SessionDesired`, and a cold one is a closed
session's tombstone. The runner's durable consumer starts from the last message of each subject,
so a runner that starts over never revives a closed session. The relay publishes the whole `SessionDesired` (ticket, one worktree with `base_ref` and
`mirror_repo`, `member`) with `Envelope.message_id` and `Nats-Msg-Id` set to the id of the event
that produced it, and `generation` set to that event's `seq`. The runner applies a hot session as
its `Session` resource and deletes a cold one (teardown); warm (hibernation) isn't built, so a
warm session stays hot. It reports `SessionStatus` on every phase change, with `message_id` and
`Nats-Msg-Id` set to the phase's id (`Session.status.phaseId`), so a report sent again is the same
message; the api records each `message_id` once. Each committed session
event also fans out, as a `rangelight.session.v1alpha1.Event` on the core NATS subject
`rl.<org>.session.<session>.events`, to the api's `WatchSession` streams.

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
- **v1alpha1** (RAN-34): `rangelight.session.v1alpha1`: `SessionService` (`ListSessions`,
  server-streaming `WatchSession`, which ends with `ABORTED` on a seq gap), and `Change`, whose
  deterministic encoding is the payload of the session event chain. The chain hashes
  `prev_hash ‖ event id ‖ at (µs) ‖ payload` (ADR 0029; encoding in `shared/rust/chain`).
- **v1alpha1** (RAN-19, additive): `SessionDesired` gains `ticket` (new message `Ticket`) and
  `member`; `Worktree` gains `base_ref` and `mirror_repo`. Together they carry everything the
  runner's `Session` resource holds.
- **v1alpha1** (RAN-37): `rangelight.auth.v1alpha1`: `AuthService.WhoAmI`, the signed-in member.
  Every RPC now answers `UNAUTHENTICATED` (HTTP 401) without a live login.
- **v1alpha1** (RAN-35, additive): `Change` gains `phase_changed` (new message `PhaseChanged`: the
  runner's `SessionStatus`, with its `message_id`), and `Session` gains `phase`. New schema
  `jsonschema/ticket.v1alpha1.schema.json`, the one ticket rule.
