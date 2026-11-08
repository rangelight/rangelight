# runner

The only Kubernetes-aware part of Rangelight (ADR 0018), in Go with controller-runtime. It runs
`Session` resources (`rangelight.dev/v1alpha1`, `deployments/helm/rangelight/files/crds/session.yaml`),
which live in the runner's own namespace, and answers the git mirror's `/auth/git`.

**A Session's lane** is the namespace `rl-<ticket key, lowercased>`, labelled
`rangelight.dev/session: <id>`, `rangelight.dev/lane: "true"`, `rangelight.dev/claude-home: "true"`
and Pod Security `restricted`. In it:

| Object | What it does |
| --- | --- |
| NetworkPolicy `lane` | no ingress; egress only to the git mirror (8080). Nothing reaches the API server. |
| CiliumNetworkPolicy `lane` | DNS to kube-dns through Cilium's proxy, answering only the mirror's service name and Anthropic's hosts (so query names can't carry data out); `toFQDNs` for `api.anthropic.com`, `claude.ai`, `platform.claude.com` (443). Without Cilium the session fails. |
| PVC `claude-home` | class `rangelight-claude-home`, label `rangelight.dev/member`; mounted as `CLAUDE_CONFIG_DIR` (ADR 0046). |
| ConfigMap `lane` | `ticket.json`, and `profile.json`: `rangelight.yaml` read once from the tip of `baseRef` in the mirror, validated against `api/jsonschema/rangelight.v1alpha1.schema.json`. |
| Pod `agent` | non-root, read-only root, no capabilities, seccomp `RuntimeDefault`, no automounted token; one projected token (audience `rangelight-git-mirror`, 600 s); bounded worktree, `/tmp` and ephemeral storage; `ndots: 1`. |

Phase: `Pending`, `Starting` (the namespace exists), `Hot` (the agent pod is Ready), or `Failed`
(an invalid profile, a namespace that isn't the session's, no Cilium) with the reason in
`status.message`. Deleting the Session deletes the namespace (finalizer `rangelight.dev/lane`).

**`/auth/git`** (port 8080): a projected token as a Basic password or Bearer token, which a
TokenReview accepts for `rangelight-git-mirror`, bound to a pod. 200 with `X-Remote-User: runner`
for the runner's own ServiceAccount (it reads profiles with that token; the mirror lets `runner`
only read), or `session-<id>` for the agent pod of the live Session `<id>`'s lane; else 401.

**Permissions.** The chart's ClusterRole lists only what a lane needs, and its
ValidatingAdmissionPolicy confines every write of the runner's ServiceAccount to lane namespaces
(`rl-*`, labelled `rangelight.dev/lane=true`), its Sessions to its own namespace, and lane pods to
the `default` ServiceAccount. The tests run the runner as that ServiceAccount, under both.

**Cluster DNS.** Lanes reach DNS only at the cluster DNS pods: by default those labelled
`k8s-app: kube-dns` in `kube-system` (CoreDNS on k3s, kubeadm, EKS and most others). The chart's one
`clusterDns` setting (`namespace`, `podLabel`) changes it for the lanes and for Dex alike (RKE2:
`k8s-app=coredns`).

Config: `configs/runner.default.toml`, then `/etc/rangelight/runner.toml`, then `RANGELIGHT_RUNNER_*`.
It sets the lane's disk sizes and `workers` (Sessions reconciled at once; a profile read is a
depth-1 clone on the runner's `/tmp` for up to 30 seconds, so `/tmp` holds up to `workers` of them).
Code: `main.go` (wiring), `internal/config/`, `internal/session/`. Deployed by the chart
(`templates/runner/`); image `build/package/docker/runner/Dockerfile`.

```sh
scripts/check.sh go     # lint, tests with envtest (-race), govulncheck; needs helm
```

The tests run against a real API server (envtest), with the chart's CRD, RBAC and admission policy,
and serve fixture repos with the real `git http-backend`; `testdata/` holds a stand-in for Cilium's
CRD. `TestDeepCopyIsGenerated` checks the generated code.
