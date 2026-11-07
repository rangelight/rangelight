#!/usr/bin/env bash
# ponytail: anonymous fetch from public upstreams until the fetch Job of Plan 4 (ADR 0047 §3).
# Usage: sync.sh <root dir> <name>=<upstream url>...   fetches each into <root>/<name>.git (created if missing).
# Explicit refspecs only, never --mirror. Upstream never writes the session namespace
# (refs/heads/rangelight and below), and the prune never removes a ref outside these refspecs.
# No gc here: session repos borrow these repos' objects (objects/info/alternates), so pruning
# unreachable objects would break them. Plan 4's maintenance must keep that rule.
set -euo pipefail
export LC_ALL=C

[[ $# -ge 2 ]] || { printf 'usage: sync.sh <root dir> <name>=<upstream url>...\n' >&2; exit 2; }
root="$1"
shift
# Anonymous and https only, unless the caller narrows it otherwise (the self-test uses file).
export GIT_ALLOW_PROTOCOL="${GIT_ALLOW_PROTOCOL:-https}" GIT_TERMINAL_PROMPT=0

failed=0
for spec in "$@"; do
  name="${spec%%=*}" upstream="${spec#*=}"
  if [[ ! "${name}" =~ ^[A-Za-z0-9][A-Za-z0-9._-]*$ || "${name}" == *.git || "${spec}" != *=?* ]]; then
    printf 'sync.sh: not <name>=<upstream url>: %s\n' "${spec}" >&2
    failed=1
    continue
  fi
  repo="${root}/${name}.git"
  [[ -d "${repo}" ]] || git init -q --bare "${repo}"
  if ! git -C "${repo}" -c credential.helper= -c gc.auto=0 -c maintenance.auto=false \
    fetch --quiet --prune --no-tags "${upstream}" \
    '+refs/heads/*:refs/heads/*' '+refs/tags/*:refs/tags/*' '^refs/heads/rangelight' '^refs/heads/rangelight/*'; then
    printf 'sync.sh: fetching %s failed\n' "${name}" >&2
    failed=1
    continue
  fi
  # HEAD follows upstream's default branch, not this image's `git init` default, so clones check it out.
  head="$(git -C "${repo}" -c credential.helper= ls-remote --symref "${upstream}" HEAD | awk '$1 == "ref:" { print $2; exit }')" ||
    head=""
  if [[ "${head}" == refs/heads/?* && "${head}" != refs/heads/rangelight && "${head}" != refs/heads/rangelight/* ]]; then
    git -C "${repo}" symbolic-ref HEAD "${head}"
  fi
done
exit "${failed}"
