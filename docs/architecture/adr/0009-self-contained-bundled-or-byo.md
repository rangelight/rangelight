# 0009. Self-contained: every dependency is bundled-capable and BYO-capable

- Status: accepted. The object-store default is pending a licence check (MinIO vs SeaweedFS vs Garage).
- Date: 2026-10-29
- Deciders: the owner

## Decision

- **Every dependency has a self-hostable open-source default that the chart can install, and can be swapped for one the org already runs.**
  - Cloud services are adapters, never requirements.
- **Support targets:** EKS, RKE2, bare metal and k3d (the quickstart).

| Need | Bundled default | BYO examples |
| --- | --- | --- |
| Postgres | CloudNativePG | RDS, existing Postgres |
| Object storage | S3-compatible (to verify) | S3, Ceph RGW |
| Secrets | OpenBao | Vault, AWS Secrets Manager (via ESO) |
| Identity | Dex | any OIDC: Okta, Entra, Keycloak |
| Registry/cache | Zot | ECR, Harbor, Artifactory |
| GitOps | Argo CD | the org's existing Argo CD |
| Workload identity | projected SA tokens | SPIRE (`scale`) |

- **No storage-system requirement.** CSI snapshots are optional (ADR 0010).

## Amendment (2026-10-30): optional subcharts; observability; the reference foundation

- **How dependencies ship:** each dependency is an **optional subchart** of the Rangelight chart.
  - On by default for the k3s single-node quickstart.
  - Off when the org brings its own (BYO).
  - The owner chose this over "prerequisites contract + separate foundation repo" and over "docs only".
- **Single node:** use k3s (k3d for laptops).
- **Observability:** Rangelight emits OTLP (traces, logs, metrics) plus a Prometheus endpoint, and never depends on a backend.
- **The reference and optional observability stack follows a stack a previous project already runs in production:**
  - VictoriaMetrics (VMCluster/VMAgent/VMAlert), VictoriaLogs and VictoriaTraces
  - a Vector agent and aggregator, with CI-tested redaction
  - OBI (OpenTelemetry eBPF Instrumentation)
  - kube-state-metrics and node-exporter
  - Grafana, provisioned from git
  - MCP servers for the stores
- **Other foundation choices to reuse as subchart defaults:**
  - RustFS for S3 (replacing the MinIO question; licence to confirm)
  - CloudNativePG with Barman
  - TopoLVM, which gives snapshots on bare metal and RKE2
  - cert-manager with approver-policy
  - Harbor/Zot
  - Dragonfly, if a cache is ever needed
  - Tetragon and Cilium as recommended, not required

## Revisit if

- A target platform, such as OpenShift, needs its own adapter set.
