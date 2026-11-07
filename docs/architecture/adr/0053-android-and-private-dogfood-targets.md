# 0053. Android as the second reference target

- Status: accepted
- Date: 2026-11-06
- Deciders: the owner
- Amends: ADR 0002 (reference targets), ARCHITECTURE.md §9 and §10, the roadmap

## Context

- Until today every target is a Kubernetes web service: the OTel Demo (ADR 0002), then Rangelight itself. Nothing in the docs mentions mobile.
- Mobile is a high-value target:
  - agent-written app code ships to users' devices
  - nobody re-runs the checks on it before release
  - the build artefact (an APK or AAB) is a single file, which is a natural in-toto subject for a receipt
- Android is the right first mobile platform:
  - Gradle and the Android SDK run on Linux, so they fit in a lane pod.
  - Unit tests and screenshot tests (Robolectric, Roborazzi) run on the JVM without an emulator, so the observed gate needs no new infrastructure.
  - iOS needs macOS hosts, which a Kubernetes cluster doesn't have.

## Decision

### 1. Android is the second reference target (public)

- **The reference app:** a fork of `android/nowinandroid` (Apache-2.0) at `rangelight/nowinandroid`. It is multi-module and uses Compose. Its Roborazzi screenshot tests and JVM unit tests already exist.
- **The lane:** the same lane as for any repo (ADR 0045). Only the toolchain image and the gate commands differ:
  - The **prebuild image** has a JDK, the Android SDK command-line tools, the pinned platform and build-tools, and a warmed Gradle cache.
  - It's built **in the cluster** from `sdkmanager` and kept in Zot only. It is **never pushed to public GHCR**, because the Android SDK licence doesn't allow redistribution.
  - **Gradle dependencies** come through the package proxy (ADR 0049). Check before Plan 9a that git-pkgs/proxy serves Maven Central and Google Maven. If it doesn't, the plan adds a Maven-capable proxy.
- **The observed gate** (ADR 0045) runs on the pushed head:
  - `./gradlew assembleDebug`, lint, the JVM unit tests and the Roborazzi verify task
  - `AC<n>` criteria are proven by tests the branch adds, as for any repo
- **The receipt subjects:** the debug APK's digest, plus the screenshot diff report as an event in the bundle.
- **Signing:**
  - Lanes only ever hold the **debug keystore**.
  - Release and Play upload keys never enter a lane, a preview or the cluster. This is the mobile form of "prod secrets never enter a preview" (ADR 0038).
  - Release signing stays in the app's own release pipeline, outside Rangelight.
- **The preview:**
  - v0.2: the APK is downloadable from the drill-in, plus the screenshot report.
  - After v0.2.0: an **emulator preview**, an Android emulator in a pod on a KVM-capable node, streamed to the browser behind the same forward-auth as web previews (ADR 0041).
  - The preflight reports whether any node exposes `/dev/kvm`.
- **When:** in v0.2.0, as Plan 9a. It needs only v0.1 capabilities (pod, prebuild, gate Job, receipt), not Argo or previews, so it can run alongside Plans 8 and 9. **Not in scope:** iOS, Play publishing, and device farms.

The maintainers also dogfood Rangelight on a private codebase.

## Consequences

- The lane contract and gate are tested against two very different toolchains (polyglot Kubernetes services, Gradle), which keeps them target-agnostic.
- Android adds the largest prebuild images so far (an SDK plus a Gradle cache, several GB) and a heavier gate. These are early stress tests for the BuildKit cache and the hot-slot caps.

## Revisit if

- git-pkgs/proxy can't serve Maven repositories.
- The emulator preview needs nested virtualisation that no supported cluster type offers. Then fall back to Gradle Managed Devices in the gate, or to an external device farm behind the gateway.
