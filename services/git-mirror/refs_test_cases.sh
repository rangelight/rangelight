# shellcheck shell=bash
# The cases of refs_test.sh (sourced by it; uses its helpers and fixtures).
# shellcheck disable=SC2153,SC2154 # W, SRV, BASE, MAIN, SELF, SECRET, MODE are set by refs_test.sh

NO_ID="rangelight: no valid identity"
NOT_ALLOWED="rangelight: ref not allowed"
READ_ONLY="is read-only; push to /session/demo.git"

t8_t9_identity_fails_closed() {
  refused "no identity: refused" "${NO_ID}" as - ls-remote "${MAIN}"
  refused "a client's own X-Remote-User is ignored" "${NO_ID}" as - -c "http.extraHeader=X-Remote-User: runner" ls-remote "${MAIN}"
  refused "a client's own X-Rangelight-User is ignored" "${NO_ID}" \
    as - -c "http.extraHeader=X-Rangelight-User: runner" ls-remote "${MAIN}"
  check "the runner's /auth/git says 401: refused with 401" \
    "[[ \$(http deny '/demo.git/info/refs?service=git-upload-pack' | head -n1) == 401 ]]"
  local bad
  for bad in "" "session-" "session-A" "session-a/../b" "session-a.b" "session" "Runner" \
    "session-$(printf 'x%.0s' $(seq 1 64))"; do
    refused "malformed identity '${bad:0:20}': refused" "${NO_ID}" as "${bad}" ls-remote "${MAIN}"
  done
  local path
  for path in /demo.git/info/refs /demo.git/packed-refs /demo.git/HEAD /demo.git/objects/info/packs /session/demo.git/config; do
    check "dumb protocol ${path}: 404 rangelight: not found" "http r '${path}' >'${TMP}/out'; [[ \$(head -n1 '${TMP}/out') == 404 ]] && grep -qF 'rangelight: not found' '${TMP}/out'"
  done
}

# git's own client: no credentials until a 401 names a scheme, then the helper's Basic password.
t8_t9_git_cli_signs_in_after_the_challenge() {
  check "a 401 names Basic, so git sends its credentials" \
    "curl -s -o /dev/null -D - -H 'X-Stub-User: deny' '${MAIN}/info/refs?service=git-upload-pack' | grep -qi '^www-authenticate: Basic realm=\"rangelight\"'"
  # deny gets the first request a 401; git's retry carries the helper's password, r (the runner).
  ok "git with a credential helper clones after the challenge" \
    git -c "http.extraHeader=X-Stub-User: deny" -c "credential.helper=!f() { echo username=session; echo password=r; }; f" ls-remote "${MAIN}"
}

# The hook on its own, without backend.sh: it must fail closed by itself.
t8_t9_hook_fails_closed_on_its_own() {
  local srv="${W}/hook" copy="${W}/hook-copy" line
  git init -q --bare "${srv}/sessions/a.git"
  cp -r "${HERE}" "${copy}"
  sed -i 's/^MAX_KIB=.*/MAX_KIB=1/' "${copy}/hooks/pre-receive"
  # shellcheck disable=SC2317,SC2329 # called through `refused` (SC2317 in shellcheck < 0.11, SC2329 after)
  hook() { (cd "${srv}/sessions/a.git" && env -u REMOTE_USER GIT_DIR=. GIT_PROJECT_ROOT="${srv}" "$@"); }
  # A real commit (of the empty tree): the hook reads what a push brings.
  local empty big
  empty="$(git -C "${srv}/sessions/a.git" commit-tree "$(git -C "${srv}/sessions/a.git" hash-object -w -t tree /dev/null)" -m empty)"
  line="${ZERO} ${empty} refs/rangelight/sessions/a/cp/1"
  ok "hook: session-a may create its own checkpoint" hook REMOTE_USER=session-a "${HERE}/hooks/pre-receive" <<<"${line}"
  refused "hook: no REMOTE_USER" "${NO_ID}" hook "${HERE}/hooks/pre-receive" <<<"${line}"
  refused "hook: empty REMOTE_USER" "${NO_ID}" hook REMOTE_USER= "${HERE}/hooks/pre-receive" <<<"${line}"
  refused "hook: REMOTE_USER with a newline" "${NO_ID}" hook REMOTE_USER=$'session-a\n' "${HERE}/hooks/pre-receive" <<<"${line}"
  refused "hook: REMOTE_USER with a slash" "${NO_ID}" hook REMOTE_USER=session-a/x "${HERE}/hooks/pre-receive" <<<"${line}"
  refused "hook: REMOTE_USER 'runner ' (a trailing space)" "${NO_ID}" hook "REMOTE_USER=runner " "${HERE}/hooks/pre-receive" <<<"${line}"
  refused "hook: runner" "rangelight: read-only identity" hook REMOTE_USER=runner "${HERE}/hooks/pre-receive" <<<"${line}"
  refused "hook: session-b in session-a's repo" "rangelight: not this session's repo" \
    hook REMOTE_USER=session-b "${HERE}/hooks/pre-receive" <<<"${line/\/a\//\/b\/}"
  refused "hook: no GIT_PROJECT_ROOT" "rangelight: not this session's repo" \
    hook GIT_PROJECT_ROOT= REMOTE_USER=session-a "${HERE}/hooks/pre-receive" <<<"${line}"
  refused "hook: malformed update line" "rangelight: malformed update" \
    hook REMOTE_USER=session-a "${HERE}/hooks/pre-receive" <<<"refs/rangelight/sessions/a/cp/1"
  refused "hook: over the size cap" "rangelight: the session repo would exceed 0 MiB" \
    hook REMOTE_USER=session-a "${copy}/hooks/pre-receive" <<<"${line}"
  sed -i -e 's/^MAX_KIB=.*/MAX_KIB=1000000/' -e 's/^MAX_REFS=.*/MAX_REFS=1/' "${copy}/hooks/pre-receive"
  ok "hook: up to the ref cap" hook REMOTE_USER=session-a "${copy}/hooks/pre-receive" <<<"${line}"
  refused "hook: over the ref cap" "rangelight: the session repo would exceed 1 refs" \
    hook REMOTE_USER=session-a "${copy}/hooks/pre-receive" <<<"${line}"$'\n'"${line/cp\/1/cp\/2}"
  # RAN-55: a file over the blob cap (here 10 bytes) is refused; GitHub would refuse it on publish.
  sed -i -e 's/^MAX_REFS=.*/MAX_REFS=10000/' -e 's/^MAX_BLOB=.*/MAX_BLOB=10/' "${copy}/hooks/pre-receive"
  ok "hook: files up to the blob cap" hook REMOTE_USER=session-a "${copy}/hooks/pre-receive" <<<"${line}"
  big="$(cd "${srv}/sessions/a.git" && printf 'more than ten bytes' | git hash-object -w --stdin)"
  big="$(cd "${srv}/sessions/a.git" && printf '100644 blob %s\tbig\n' "${big}" | git mktree | xargs -I{} git commit-tree {} -m big)"
  refused "hook: a file over the blob cap" "rangelight: a file over 0 MiB" \
    hook REMOTE_USER=session-a "${copy}/hooks/pre-receive" <<<"${ZERO} ${big} refs/heads/rangelight/a/work"
  chmod -x "${copy}/hooks/pre-receive"
  ok "backend.sh runs on its own" env REMOTE_USER=session-a GIT_PROJECT_ROOT="${srv}" PATH_INFO=/demo.git/info/refs \
    QUERY_STRING=service=git-upload-pack "${copy}/backend.sh"
  check "backend.sh: a non-executable pre-receive is a 500, not a push without checks" \
    "grep -q '^Status: 500' '${TMP}/out' && grep -qF 'rangelight: the push hook is not executable' '${TMP}/out'"
  local ref
  for ref in refs/rangelight/sessions/a refs/rangelight/sessions/a/ refs/rangelight/sessions/ab/cp/1 \
    refs/heads/rangelight/a refs/heads/rangelight/a/ refs/heads/rangelight/ab/x refs/rangelight/a/x \
    refs/heads/main refs/tags/v2 refs/heads/a/x; do
    refused "hook: session-a may not update ${ref}" "${NOT_ALLOWED}: ${ref}" \
      hook REMOTE_USER=session-a "${HERE}/hooks/pre-receive" <<<"${ZERO} 1111111111111111111111111111111111111111 ${ref}"
  done
}

t8_t9_ac1_session_pushes_into_its_own_repo() {
  ok "AC1: session-a pushes its checkpoint and its branch" \
    as a -C "${W}/a" push -q "${SELF}" HEAD:refs/rangelight/sessions/a/cp/1 HEAD:refs/heads/rangelight/a/work
  check "AC1: both refs are in sessions/a.git" \
    "[[ \$(ref_in sessions/a.git refs/rangelight/sessions/a/cp/1) == ${SECRET} ]] && ref_in sessions/a.git refs/heads/rangelight/a/work >/dev/null"
  check "AC1: sessions/a.git borrows demo.git's objects" \
    "[[ \$(<'${SRV}/sessions/a.git/objects/info/alternates') == '${SRV}/demo.git/objects' ]]"
  check "AC1: demo.git holds no session ref and no session object" \
    "[[ -z \$(git -C '${SRV}/demo.git' for-each-ref refs/rangelight refs/heads/rangelight) ]] && ! git -C '${SRV}/demo.git' cat-file -e ${SECRET} 2>/dev/null"
  check "post-receive reports the checkpoint" \
    "grep -qF '\"session\":\"a\",\"checkpoint\":1,\"commit\":\"${SECRET}\"' '${W}/events'"
  check "a push without the session's Authorization header tells the runner nothing" "[[ ! -e '${W}/pushed' ]]"
  # RAN-55: a work-branch push reaches the runner with the pushing session's own token (Basic password "a").
  local work token
  work="$(git -C "${W}/a" commit-tree 'HEAD^{tree}' -p HEAD -m work)" token="Basic $(printf 'x:a' | base64)"
  ok "RAN-55: session-a pushes its work branch and another branch, with its token" \
    as - -c "http.extraHeader=Authorization: ${token}" -C "${W}/a" push -q "${SELF}" \
    "${work}:refs/heads/rangelight/a/work" "${work}:refs/heads/rangelight/a/other" "${work}:refs/rangelight/sessions/a/cp/3"
  check "RAN-55: post-receive POSTs the work branch only, with the session's own Authorization header" \
    "[[ \$(<'${W}/pushed') == '${token} {\"session\":\"a\",\"ref\":\"refs/heads/rangelight/a/work\",\"commit\":\"${work}\"}' ]]"
  ok "session-b pushes its own checkpoint" as b -C "${W}/b" push -q "${SELF}" HEAD:refs/rangelight/sessions/b/cp/1
  git init -q --bare "${SRV}/other.git"
  refused "a session repo stays bound to the repo it started from" "rangelight: this session belongs to another repo" \
    as a -C "${W}/a" push "${BASE}/session/other.git" HEAD:refs/heads/rangelight/a/elsewhere
  refused "a session that hasn't pushed has no repo to fetch" "rangelight: this session has not pushed yet" \
    as session-c ls-remote "${SELF}"
}

t8_t9_ac2_session_cannot_push_main() {
  refused "AC2: session-a pushing refs/heads/main is rejected" "${NOT_ALLOWED}: refs/heads/main" \
    as a -C "${W}/a" push "${SELF}" HEAD:refs/heads/main
  refused "AC2: a push mixing an own ref and main is rejected whole" "${NOT_ALLOWED}: refs/heads/main" \
    as a -C "${W}/a" push "${SELF}" HEAD:refs/rangelight/sessions/a/cp/2 HEAD:refs/heads/main
  check "AC2: cp/2 was not created" "! ref_in sessions/a.git refs/rangelight/sessions/a/cp/2 >/dev/null"
  refused "AC2: pushing to demo.git itself is refused" "${READ_ONLY}" as a -C "${W}/a" push "${MAIN}" HEAD:refs/heads/main
  refused "AC2: deleting main in demo.git is refused" "${READ_ONLY}" as a -C "${W}/a" push "${MAIN}" :refs/heads/main
  refused "AC2: a tag is rejected" "${NOT_ALLOWED}: refs/tags/v2" as a -C "${W}/a" push "${SELF}" HEAD:refs/tags/v2
  refused "AC2: refs/heads/rangelight/a (no slash) is rejected" "${NOT_ALLOWED}: refs/heads/rangelight/a" \
    as a -C "${W}/a" push "${SELF}" HEAD:refs/heads/rangelight/a
}

t8_t9_checkpoints_are_append_only() {
  local a="${W}/a"
  commit_in "${a}" step-2
  ok "a checkpoint may move forward" as a -C "${a}" push -q "${SELF}" HEAD:refs/rangelight/sessions/a/cp/9
  refused "a checkpoint can't be rewound" "rangelight: checkpoints are append-only, can't rewind refs/rangelight/sessions/a/cp/9" \
    as a -C "${a}" push -f "${SELF}" HEAD~1:refs/rangelight/sessions/a/cp/9
  refused "a checkpoint can't be deleted" "rangelight: checkpoints are append-only, can't delete refs/rangelight/sessions/a/cp/1" \
    as a -C "${a}" push "${SELF}" :refs/rangelight/sessions/a/cp/1
  ok "a session branch may be force-pushed" as a -C "${a}" push -q -f "${SELF}" v1:refs/heads/rangelight/a/work
  check "the forced branch moved" "[[ \$(ref_in sessions/a.git refs/heads/rangelight/a/work) == \$(git -C '${a}' rev-parse v1) ]]"
  git -C "${a}" reset -q --hard HEAD~1
}

# One push can't create unbounded refs (each is a file), and no malformed object gets stored.
test_push_is_capped_and_fscked() {
  local specs=() i blob tree commit
  for i in $(seq 1 1001); do specs+=("HEAD:refs/heads/rangelight/a/many/${i}"); done
  refused "a push with 1001 ref updates is refused" "rangelight: too many ref updates in one push (over 1000)" \
    as a -C "${W}/a" push "${SELF}" "${specs[@]}"
  check "none of those refs was written" "[[ -z \$(git -C '${SRV}/sessions/a.git' for-each-ref refs/heads/rangelight/a/many) ]]"
  blob="$(git -C "${W}/a" hash-object -w --stdin <<<"x")"
  tree="$(python3 -I -c 'import sys; sys.stdout.buffer.write(b"100644 .git\0" + bytes.fromhex(sys.argv[1]))' "${blob}" |
    git -C "${W}/a" hash-object -t tree --literally -w --stdin)"
  commit="$(git -C "${W}/a" commit-tree -m bad-tree "${tree}")"
  refused "a tree with a .git entry is refused (fsck)" "hasDotgit" \
    as a -C "${W}/a" push "${SELF}" "${commit}:refs/heads/rangelight/a/bad"
  check "the bad tree was not stored" "! git -C '${SRV}/sessions/a.git' cat-file -e ${tree} 2>/dev/null"
}

t8_t9_ac3_session_cannot_push_another_sessions_refs() {
  local before
  before="$(ref_in sessions/b.git refs/rangelight/sessions/b/cp/1)"
  refused "AC3: session-a pushing refs/rangelight/sessions/b/cp/1 is rejected" "${NOT_ALLOWED}: refs/rangelight/sessions/b/cp/1" \
    as a -C "${W}/a" push -f "${SELF}" HEAD:refs/rangelight/sessions/b/cp/1
  refused "AC3: session-a pushing refs/heads/rangelight/b/work is rejected" "${NOT_ALLOWED}: refs/heads/rangelight/b/work" \
    as a -C "${W}/a" push "${SELF}" HEAD:refs/heads/rangelight/b/work
  refused "AC3: session-a can't address b's repo" "rangelight: not your repo" as a ls-remote "${BASE}/sessions/b.git"
  check "AC3: b's repo is unchanged" "[[ \$(ref_in sessions/b.git refs/rangelight/sessions/b/cp/1) == ${before} ]]"
}

t8_t9_ac4_session_cannot_read_another_session() {
  local v unadvertised
  for v in 0 1 2; do
    # A v0/v1 client won't even ask for an id it wasn't shown; in v2 the server says it has no such ref.
    unadvertised="not our ref ${SECRET}"
    [[ "${v}" == 2 ]] || unadvertised="Server does not allow request for unadvertised object ${SECRET}"
    ok "AC4: ls-remote demo.git as session-b (v${v})" as b -c "protocol.version=${v}" ls-remote "${MAIN}"
    check "AC4: demo.git lists nothing of session a (v${v})" "! grep -qE '/a/|${SECRET}' '${TMP}/out' && grep -q refs/heads/main '${TMP}/out'"
    ok "AC4: ls-remote its own repo as session-b (v${v})" as b -c "protocol.version=${v}" ls-remote "${SELF}"
    check "AC4: its repo lists its own checkpoint and nothing of a (v${v})" \
      "grep -q refs/rangelight/sessions/b/cp/1 '${TMP}/out' && ! grep -qE '/a/|${SECRET}' '${TMP}/out'"
    refused "AC4: fetching a's checkpoint by name fails (v${v})" "couldn't find remote ref" \
      as b -c "protocol.version=${v}" -C "${W}/b" fetch "${SELF}" refs/rangelight/sessions/a/cp/1
    refused "AC4: fetching a's checkpoint by object id fails (v${v})" "${unadvertised}" \
      as b -c "protocol.version=${v}" -C "${W}/b" fetch "${SELF}" "${SECRET}"
    refused "AC4: fetching it from demo.git by object id fails (v${v})" "${unadvertised}" \
      as b -c "protocol.version=${v}" -C "${W}/b" fetch "${MAIN}" "${SECRET}"
  done
  check "AC4: no store b can read (its repo, with demo.git through alternates) has a's commit" \
    "! git -C '${SRV}/sessions/b.git' cat-file -e ${SECRET} 2>/dev/null"
  check "AC4: b never got a's commit" "! git -C '${W}/b' cat-file -e ${SECRET} 2>/dev/null"
}

# The review's attack: a ref to another session's object id, with an empty pack.
t8_t9_foreign_object_id_push_yields_nothing() {
  local cmd="${ZERO} ${SECRET} refs/heads/rangelight/b/stolen" caps="report-status"
  {
    printf '%04x%s\0%s' $((4 + ${#cmd} + 1 + ${#caps})) "${cmd}" "${caps}"
    printf 0000
    python3 -I -c 'import hashlib, sys; h = b"PACK\0\0\0\2\0\0\0\0"; sys.stdout.buffer.write(h + hashlib.sha1(h).digest())'
  } >"${W}/attack"
  curl -s -H 'X-Stub-User: b' -H 'Content-Type: application/x-git-receive-pack-request' \
    --data-binary @"${W}/attack" "${SELF}/git-receive-pack" >"${TMP}/out" 2>&1 || true
  check "attack: a ref to a foreign object id is refused (missing objects)" \
    "grep -qa 'ng refs/heads/rangelight/b/stolen missing necessary objects' '${TMP}/out'"
  check "attack: no such ref in b's repo" "! ref_in sessions/b.git refs/heads/rangelight/b/stolen >/dev/null"
  refused "attack: so there is nothing to fetch" "couldn't find remote ref" \
    as b -C "${W}/b" fetch "${SELF}" refs/heads/rangelight/b/stolen
}

t8_t9_ac5_runner_reads_and_never_pushes() {
  ok "AC5: runner fetches refs/heads/main" as r -C "${W}/b" fetch -q "${MAIN}" refs/heads/main
  ok "the runner reads a session's repo" as r ls-remote "${BASE}/sessions/a.git"
  check "the runner sees session a's checkpoint" "grep -q refs/rangelight/sessions/a/cp/1 '${TMP}/out'"
  refused "AC5: runner pushing main is rejected" "${READ_ONLY}" as r -C "${W}/a" push "${MAIN}" HEAD:refs/heads/main
  refused "AC5: runner pushing into a session's repo is rejected" "rangelight: the runner pushes nothing" \
    as r -C "${W}/a" push "${BASE}/sessions/a.git" HEAD:refs/rangelight/sessions/a/cp/7
  refused "AC5: the runner has no session repo of its own" "rangelight: not your repo" \
    as r -C "${W}/a" push "${SELF}" HEAD:refs/heads/runner-x
  check "AC5: nothing the runner pushed landed" \
    "! ref_in sessions/a.git refs/rangelight/sessions/a/cp/7 >/dev/null && ! ref_in demo.git refs/heads/runner-x >/dev/null"
}

# Over http.postBuffer (1 MiB), git sends the body chunked, without a Content-Length.
test_big_push_goes_chunked() {
  head -c 3000000 /dev/urandom >"${W}/a/big.bin"
  git -C "${W}/a" add big.bin
  git -C "${W}/a" commit -qm big
  ok "a 3 MB push (chunked) is accepted" as a -C "${W}/a" push -q "${SELF}" HEAD:refs/heads/rangelight/a/big
  [[ "${MODE}" != cgi ]] || check "the push really was chunked" "grep -q git-receive-pack '${W}/chunked'"
}

test_ac6_sync_keeps_session_refs() {
  local up="${W}/upstream" main="${SRV}/demo.git" only cp
  only="$(git -C "${up}" rev-parse v1)"
  # demo.git never gets these over HTTP; set here only to prove the sync leaves them alone.
  git -C "${main}" update-ref refs/heads/rangelight/a/only-here "${only}"
  git -C "${main}" update-ref refs/rangelight/sessions/a/cp/1 "${only}"
  cp="$(ref_in sessions/a.git refs/rangelight/sessions/a/cp/1)"
  commit_in "${up}" upstream-moved
  git -C "${up}" branch -D -q old
  git -C "${up}" branch rangelight/a/work v1
  ok "AC6: sync.sh runs" env GIT_ALLOW_PROTOCOL=file "${HERE}/sync.sh" "${SRV}" "demo=file://${up}"
  git -C "${up}" branch -D -q rangelight/a/work
  git -C "${up}" branch rangelight v1
  ok "AC6: sync.sh runs again" env GIT_ALLOW_PROTOCOL=file "${HERE}/sync.sh" "${SRV}" "demo=file://${up}"
  check "AC6: a session-only branch not upstream survives --prune" "[[ \$(ref_in demo.git refs/heads/rangelight/a/only-here) == ${only} ]]"
  check "AC6: refs/rangelight/sessions/a/cp/1 survives" "[[ \$(ref_in demo.git refs/rangelight/sessions/a/cp/1) == ${only} ]]"
  check "AC6: upstream's rangelight and rangelight/* branches are not fetched" \
    "! ref_in demo.git refs/heads/rangelight >/dev/null && ! ref_in demo.git refs/heads/rangelight/a/work >/dev/null"
  check "AC6: the session's own repo is untouched" "[[ \$(ref_in sessions/a.git refs/rangelight/sessions/a/cp/1) == ${cp} ]]"
  check "AC6: main follows upstream" "[[ \$(ref_in demo.git refs/heads/main) == \$(git -C '${up}' rev-parse main) ]]"
  check "AC6: a branch deleted upstream is pruned" "! ref_in demo.git refs/heads/old >/dev/null"
  git -C "${up}" branch -q trunk main
  git -C "${up}" symbolic-ref HEAD refs/heads/trunk
  ok "AC6: sync.sh creates a new repo" env GIT_ALLOW_PROTOCOL=file "${HERE}/sync.sh" "${SRV}" "fresh=file://${up}"
  check "AC6: a new repo's HEAD is upstream's default branch" \
    "[[ \$(git -C '${SRV}/fresh.git' symbolic-ref HEAD) == refs/heads/trunk ]]"
  git -C "${up}" symbolic-ref HEAD refs/heads/rangelight
  ok "AC6: sync.sh runs with upstream's HEAD on a session branch" env GIT_ALLOW_PROTOCOL=file "${HERE}/sync.sh" "${SRV}" "fresh=file://${up}"
  check "AC6: HEAD never follows upstream into the session namespace" \
    "[[ \$(git -C '${SRV}/fresh.git' symbolic-ref HEAD) == refs/heads/trunk ]]"
  git -C "${up}" symbolic-ref HEAD refs/heads/main
  refused "sync.sh refuses a non-https upstream by default" "transport 'file' not allowed" \
    "${HERE}/sync.sh" "${SRV}" "demo=file://${up}"
  local spec
  for spec in "../x=file://${up}" "demo.git=file://${up}" "demo=" "demo"; do
    refused "sync.sh refuses the repo spec '${spec/file:\/\/${up}/<upstream>}'" "sync.sh: not <name>=<upstream url>: ${spec}" \
      env GIT_ALLOW_PROTOCOL=file "${HERE}/sync.sh" "${SRV}" "${spec}"
  done
}

# nginx only. Per identity (10 r/s, burst 50): one session, even from many addresses, can't starve
# the others. Per address (20 r/s, burst 50), before auth: a flood never reaches /auth/git past it.
test_limits_hold_back_one_identity_and_one_address() {
  local url="${MAIN}/info/refs?service=git-upload-pack" args=() i before after denied
  for i in $(seq 1 120); do
    args+=(${args[0]+--next} -s -o /dev/null -w '%{http_code}\n' -H 'X-Stub-User: a' -H "X-Test-Client-IP: 10.1.0.${i}" "${url}")
  done
  curl "${args[@]}" >"${TMP}/out"
  check "limits: session-a from 120 addresses gets 429" "grep -qx 429 '${TMP}/out'"
  check "limits: session-b is still served meanwhile" "[[ \$(http b /demo.git/info/refs?service=git-upload-pack | head -n1) == 200 ]]"
  args=()
  for i in $(seq 1 120); do
    args+=(${args[0]+--next} -s -o /dev/null -w '%{http_code}\n' -H 'X-Stub-User: deny' -H 'X-Test-Client-IP: 10.2.0.1' "${url}")
  done
  before="$(wc -l <"${W}/auth-calls")"
  curl "${args[@]}" >"${TMP}/out"
  after="$(wc -l <"${W}/auth-calls")" denied="$(grep -cx 401 "${TMP}/out" || true)"
  check "limits: an unauthenticated flood from one address gets 429" "grep -qx 429 '${TMP}/out'"
  check "limits: only the requests let through reached /auth/git (${denied} of 120)" "(( ${after} - ${before} == ${denied} && ${denied} < 120 ))"
}

# RAN-55: publish.sh, the runner's publish Job, copies one session's work branch and nothing else from
# /sessions/<id>.git (as the runner) to its upstream. Through nginx it runs in the mirror's image, as the Job does.
test_publish_copies_only_the_work_branch() {
  local id=019a5e1c-0000-7000-8000-000000000055 none=019a5e1c-0000-7000-8000-000000000056
  local up="${W}/publish-up.git" stuck="${W}/publish-stuck.git" repo="${SRV}/sessions/${id}.git" work
  printf 'r\n' >"${W}/mirror-token" # the runner, to the stub /auth/git
  printf 'ghs_test\n' >"${W}/github-token"
  git init -q --bare "${up}"
  git init -q --bare "${repo}"
  work="$(git -C "${W}/a" commit-tree 'HEAD^{tree}' -p HEAD -m publish)"
  git -C "${W}/a" push -q "${repo}" "${work}:refs/heads/rangelight/${id}/work" "${work}:refs/heads/rangelight/${id}/other" \
    "${work}:refs/heads/main"
  ok "publish.sh publishes the work branch" publish "${id}" "file://${up}"
  check "only the work branch reaches upstream, at the pushed commit" \
    "[[ \$(git --git-dir '${up}' for-each-ref --format='%(refname) %(objectname)') == 'refs/heads/rangelight/${id}/work ${work}' ]]"
  check "the termination log says what was published" "[[ \$(<'${W}/termination-log') == 'published ${work}' ]]"
  # A session repo without a work branch: nothing to publish. No session repo at all (a 404): a failure.
  git init -q --bare "${SRV}/sessions/${none}.git"
  git -C "${W}/a" push -q "${SRV}/sessions/${none}.git" "${work}:refs/heads/rangelight/${none}/other"
  ok "a session without a work branch publishes nothing" publish "${none}" "file://${up}"
  check "the termination log says so" "[[ \$(<'${W}/termination-log') == 'nothing to publish' ]]"
  refused "a mirror that has no repo for the session is a failure, not nothing" "fetching the work branch from the mirror failed" \
    publish 019a5e1c-0000-7000-8000-000000000057 "file://${up}"
  # An upstream that refuses a workflow change the way GitHub does: exit 3, which the Job never retries.
  git init -q --bare "${stuck}"
  # shellcheck disable=SC2016 # GitHub's message, backticks and all
  printf '#!/bin/sh\necho "refusing to allow a GitHub App to create or update workflow \\`.github/workflows/ci.yaml\\` without \\`workflows\\` permission" >&2\nexit 1\n' \
    >"${stuck}/hooks/pre-receive"
  chmod +x "${stuck}/hooks/pre-receive"
  local code=0
  publish "${id}" "file://${stuck}" >"${TMP}/out" 2>&1 || code=$?
  check "a workflow change GitHub refuses exits 3" "[[ ${code} -eq 3 ]]"
  check "and says why" "grep -qF 'changes .github/workflows' '${W}/termination-log'"
  refused "publish.sh refuses a session id that isn't one" "usage: publish.sh" publish "../x" "file://${up}"
}

# publish <id> <upstream>: publish.sh against this mode's mirror, with the test's two token files.
publish() {
  local env=(RANGELIGHT_MIRROR_TOKEN_FILE="${W}/mirror-token" RANGELIGHT_GITHUB_TOKEN_FILE="${W}/github-token"
    RANGELIGHT_TERMINATION_LOG="${W}/termination-log" GIT_ALLOW_PROTOCOL=file:http)
  if [[ "${MODE}" == nginx ]]; then
    docker run --rm "${env[@]/#/--env=}" "${IN_MIRROR[@]}" "${HERE}/publish.sh" "$1" "${BASE}" "$2"
  else
    env "${env[@]}" "${HERE}/publish.sh" "$1" "${BASE}" "$2"
  fi
}
