# runner

The only Kubernetes-aware part of Rangelight (ADR 0018), in Go with controller-runtime. It runs
`Session` resources (`rangelight.dev/v1alpha1`, `deployments/helm/rangelight/files/crds/session.yaml`),
which live in the runner's own namespace, and answers the git mirror's `/auth/git`.

**The runner protocol** (`api/README.md`, ADR 0021). A durable JetStream pull consumer
(`runner-<runner_id>`) of `rl.<org>.runner.<runner_id>.desired.*`, one subject per session, from the
last message of each (so a runner that starts over never revives a closed session), one message at a time: a hot
`SessionDesired` is server-side applied as the Session named by its id (field manager
`rangelight-runner`), a cold one deletes it. Acked only after that write; terminated, with the
reason logged, when it can never succeed (undecodable, another runner's, an invalid ticket the API
server refuses). An older `generation` than `spec.generation` is ignored. Each phase change gets a
new `status.phaseId`, and the reconciler hands the settled Session to a publisher that sends
`SessionStatus` on `rl.<org>.runner.<runner_id>.status` (`Nats-Msg-Id` = `phaseId`), keeping the
latest per session and retrying while NATS is away. NATS reconnects and retries back off from
0.5s to 30s with jitter; lanes never wait on NATS, and `/healthz` reflects Kubernetes only.

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

Phase: `Pending`, `Starting` (the namespace exists, the agent container not Ready yet: an exit, a
restart or ImagePullBackOff is in `status.message`), `Hot` (only while the agent container is
Ready), or `Failed` (an invalid profile, a namespace that isn't the session's, no Cilium, or an agent
container in CrashLoopBackOff after 3 restarts: the last exit's reason and code) with the reason in
`status.message`; reported as `QUEUED`, `STARTING`, `RUNNING`, `FAILED`, and `GONE` once deleted. Deleting the Session deletes the namespace (finalizer `rangelight.dev/lane`).

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
`nats_url`, `org` and `runner_id` name the runner protocol's server and subjects; the desired stream is
`rl-<org>-desired`, by name. The runner signs in as itself (`internal/natsconn`, ADR 0021 amendment): its
projected token (`nats_token_file`, audience `rangelight-nats`) is read at every connect, and its replies come
under `_INBOX_runner-<runner_id>`, so it may consume only its own sessions and report only its own status.
`nats_url` is `tls://`, or `nats://` to a Service in the same cluster: a remote runner needs TLS. Each lane's agent
container gets a projected token of its own for NATS, which lets it reach only its own terminal.
It sets the lane's disk sizes and `workers` (Sessions reconciled at once; a profile read is a
depth-1 clone on the runner's `/tmp` for up to 30 seconds, so `/tmp` holds up to `workers` of them).
Code: `main.go` (wiring), `internal/config/`, `internal/session/` (`protocol.go`, `desired.go`,
`status.go`: the runner protocol). Deployed by the chart
(`templates/runner/`); image `build/package/docker/runner/Dockerfile`.

```sh
scripts/check.sh go     # lint, tests with envtest (-race), govulncheck; needs helm
```

The tests run against a real API server (envtest), with the chart's CRD, RBAC and admission policy,
a real nats-server (in-process, pinned by `go.mod`), and serve fixture repos with the real
`git http-backend`; `testdata/` holds a stand-in for Cilium's
CRD. `TestDeepCopyIsGenerated` checks the generated code.
