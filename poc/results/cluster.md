# Dev cluster: preflight and smoke results (RAN-16)

- Date: 2026-11-07
- Cluster: the owner's single-node dev cluster. RKE2 v1.37.0, containerd 2.3, Cilium, the local-path
  StorageClass as the default. It was used as it was, with no cluster created or deleted.
- Commands: `RANGELIGHT_KUBE_CONTEXT=<context> scripts/cluster.sh <command>`.
  Versions are pinned in `deployments/dev/versions.env`.

## Preflight

`scripts/cluster.sh preflight` exited 0:

```text
PASS  Kubernetes 1.37 (>= 1.32)
PASS  egress NetworkPolicy enforced: the deny-egress pod can't reach kubernetes.default.svc, the open pod can
PASS  Pod Security rejects a privileged pod (restricted)
PASS  OIDC discovery and JWKS reachable from a pod
PASS  a 1 GiB PVC binds (default StorageClass) and is writable
INFO  gVisor RuntimeClass: none (optional, ADR 0015)
INFO  admission policy rangelight-agent-guard: present
```

How each line is proven:

- **Egress:** the probe pod under a deny-egress NetworkPolicy (DNS still allowed) has to end with
  a blocked connection (curl exit 7 or 28). A twin pod without the policy has to reach
  `kubernetes.default.svc`. This tests behaviour, not the CNI's name. A copy of the script with the
  policy's selector pointed elsewhere reported `FAIL  egress NetworkPolicy not enforced`.
- **Pod Security:** a privileged pod is sent with `--dry-run=server` into the temporary
  `restricted` namespace. Only a `PodSecurity` denial counts as a pass.
- **OIDC:** a pod uses its ServiceAccount token to fetch `/.well-known/openid-configuration` and then
  `/openid/v1/jwks`.
- **PVC:** a 1 GiB claim on the default StorageClass is bound, and a pod writes to it.
- **Cleanup:** the temporary namespace `rangelight-preflight-<random>` is deleted on exit. That was
  checked after a pass, after a failure (minimum version raised to 1.99) and after SIGTERM mid-run.

## Smoke

Before `up` (exit 1):

```text
FAIL  namespace rangelight-system does not exist (run: scripts/cluster.sh up)
cluster.sh: smoke: 1 check(s) failed
```

`scripts/cluster.sh up && scripts/cluster.sh smoke` (exit 0):

```text
PASS  namespace rangelight-system exists
```

There is no registry until a ticket needs one (RAN-32). The Zot that RAN-16 installed was removed
with `scripts/cluster.sh down`; `kubectl get all,pvc -n rangelight-system` now shows no resources.
