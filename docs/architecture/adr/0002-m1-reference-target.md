# 0002. M1 reference target: the OpenTelemetry Demo, then dogfooding

- Status: accepted
- Date: 2026-10-29
- Deciders: the owner

## Context

The M1 walking skeleton and the launch demo need a target codebase. It must be:
- public and reproducible by anyone on k3d
- multi-service, so we can preview only the services a ticket touches
- Helm-native, so Argo CD renders the org's own chart
- built with a real UI for the Preview panel and for Playwright proof

## Options

1. **OpenTelemetry Demo** (Astronomy Shop):
   - a dozen-plus services in about ten languages
   - an official Helm chart
   - a TypeScript storefront, flagd-ui, Grafana and Jaeger
   - Python Agent, Chatbot and MCP services
2. A private codebase of the maintainers: not public, and it would be removed.
3. Rangelight building itself: chicken-and-egg in M1.
4. The OTel Demo for M1 and launch, then dogfooding from M2.

## Decision

Option 4.
- M1 targets a fork of `open-telemetry/opentelemetry-demo` as the reference org.
- From M2, Rangelight's own tickets run through Rangelight.

## Consequences

- The `rangelight.yaml` derivation and the preview overlays must handle a real third-party chart we don't control.
- Playwright proof drives the storefront UI.
- The polyglot services stress the per-repo toolchains, devcontainer images and build caches early. That is a good test.
- Rangelight's own OTel output can be shown in the demo's own Grafana and Jaeger.
- The demo's Agent and MCP services give MCP-gateway and policy scenarios on real code.

## Revisit if

- The upstream demo's chart becomes too heavy for a laptop k3d cluster even with preview overlays.

## Amendment (2026-11-06)

- **Android is the second reference target:** a fork of `android/nowinandroid`, in v0.2.0 as Plan 9a (ADR 0053 §1).
- **The private codebase comes back as a private dogfood target** from Plan 4, under strict no-leak rules. That replaces option 2's "it will be removed".
