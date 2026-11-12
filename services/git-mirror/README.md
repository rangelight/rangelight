# git-mirror

The in-cluster git server (ADR 0011): stock `git http-backend` behind nginx and fcgiwrap, plus
a few scripts. Sessions push checkpoints and branches here, and never hold an upstream credential.

## Contract

nginx asks the runner's `/auth/git` (RAN-19) about every request. A 2xx answer carries the caller's
identity in `X-Remote-User`: `runner`, or `session-<id>` (`<id>`: 1–63 of `[a-z0-9-]`, starting
with `[a-z0-9]`). Anything else, or no answer, is refused. `backend.sh` and the hooks check it again.

| URL | Repo on disk | Who | What |
| --- | --- | --- | --- |
| `/<repo>.git` | `<repo>.git`: upstream refs only | any identity | fetch |
| `/session/<repo>.git` | `sessions/<own id>.git` | that session | fetch, push its own refs |
| `/sessions/<id>.git` | `sessions/<id>.git` | the runner | fetch |

- **One repo per session.** It is created on the session's first push. It borrows `<repo>.git`'s
  objects (`objects/info/alternates`) but writes only its own, so no session can name, reach or
  fetch another session's refs or objects. The path comes from the identity, never from the URL.
- **What a session may push** (`hooks/pre-receive`), in its own repo only, at most 1000 ref updates
  per push, while the repo stays under 512 MiB and 10 000 refs, no file over 100 MiB (GitHub's limit), and
  only objects that pass fsck:
  - `refs/rangelight/sessions/<id>/…`: checkpoints, append-only (no delete, no rewind);
  - `refs/heads/rangelight/<id>/…`: its branches (force-push and delete allowed).
- **The runner** is the trusted control plane. It may read every repo and push none.
- **Upstream** reaches `<repo>.git` only through `sync.sh`: explicit refspecs, never `--mirror`,
  never `refs/heads/rangelight*`, and no gc (session repos depend on these objects).
- **Limits** (`nginx.conf`):
  - per client address, before auth: 20 requests/s (burst 50) and 8 at a time;
  - per identity: 10 requests/s (burst 50) and 4 at a time;
  - 100 MiB per push; 15 s to send the request headers and between body reads.
- **Network** (the chart): only namespaces labelled `rangelight.dev/lane: "true"` and the
  `rangelight-runner` namespace may connect.
- `hooks/post-receive` POSTs each new checkpoint to `RANGELIGHT_EVENTS_URL` when that is set, and each
  push of a session's `refs/heads/rangelight/<id>/work` to the runner's `/git/pushed`, with the pushing
  session's own Authorization header, so the runner publishes it (the mirror itself never pushes upstream).
- `publish.sh` is not the mirror's: it is the runner's publish Job, run from this image because it has git.
  It copies one session's work branch from `/sessions/<id>.git` to its upstream.

## Files

`nginx.conf` (it asks the runner in `RANGELIGHT_RUNNER_NAMESPACE`, default `rangelight-runner`), `backend.sh`
(the CGI program), `identity.sh`, `hooks/`, `publish.sh`, `sync.sh` (ponytail: until
Plan 4's fetch Job). Image: `build/package/docker/git-mirror/`. Kubernetes:
`deployments/helm/rangelight/templates/git-mirror/`.

## Tests

```sh
services/git-mirror/refs_test.sh   # also run by: scripts/check.sh hooks
```

The suite runs once through a stand-in for nginx (`refs_test_server.py`), then again through real
nginx + fcgiwrap if both are installed. CI requires both (`RANGELIGHT_NGINX_REQUIRED=true`).
