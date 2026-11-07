#!/usr/bin/env bash
# Real Postgres and NATS for the Rust tests, without Docker or system packages (ADR 0049 §8).
# Downloads pinned, checksum-verified release binaries into a cache, starts both in a temp dir
# (Postgres on a Unix socket, NATS on a random port), runs the command, and tears them down.
# Usage: scripts/test/services.sh run <command...>
set -euo pipefail
shopt -s inherit_errexit

NATS_VERSION="2.15.2"
POSTGRES_VERSION="18.6.1"
CACHE="${XDG_CACHE_HOME:-${HOME}/.cache}/rangelight/test-services"

# platform: the release names of this machine, or fail.
platform() {
  case "$(uname -s)-$(uname -m)" in
    Linux-x86_64) printf 'amd64 x86_64-unknown-linux-gnu\n' ;;
    Linux-aarch64) printf 'arm64 aarch64-unknown-linux-gnu\n' ;;
    *) printf 'services: no pinned binaries for %s\n' "$(uname -sm)" >&2; return 1 ;;
  esac
}

# sha256_of <archive>: the pinned checksum.
sha256_of() {
  case "$1" in
    "nats-server-v${NATS_VERSION}-linux-amd64.tar.gz") printf '67cb8e10bc6f0c4b47f75a728529a20953dbae2a4f9a4a52ae8517d65ae303ff' ;;
    "nats-server-v${NATS_VERSION}-linux-arm64.tar.gz") printf 'bc2c788dcbf2849af749564e06a0d48c19bced851f2d68a2e21c54d078949b28' ;;
    "postgresql-${POSTGRES_VERSION}-x86_64-unknown-linux-gnu.tar.gz") printf 'e84b02dd8d0ce5879fb697330a789231c8b8f8d958dae3ad4d90fecc710b1315' ;;
    "postgresql-${POSTGRES_VERSION}-aarch64-unknown-linux-gnu.tar.gz") printf '1ab47fa2f06ff70454309ad02e5d89c46488300d43977231481fd6b160b9d556' ;;
    *) return 1 ;;
  esac
}

# fetch <url>: download into the cache once, verify, unpack; print the unpacked directory.
fetch() {
  local url="$1" archive="${1##*/}" dir
  dir="${CACHE}/${archive%.tar.gz}"
  if [[ ! -d "${dir}" ]]; then
    mkdir -p "${CACHE}"
    curl -sSfL --retry 3 -o "${dir}.tar.gz.part" "${url}"
    printf '%s  %s\n' "$(sha256_of "${archive}")" "${dir}.tar.gz.part" | sha256sum -c --quiet - >&2
    mkdir -p "${dir}.part"
    tar -xzf "${dir}.tar.gz.part" -C "${dir}.part" --strip-components 1
    mv "${dir}.part" "${dir}"
    rm -f "${dir}.tar.gz.part"
  fi
  printf '%s' "${dir}"
}

# start <tmp>: start both servers under tmp; print the environment the tests read.
start() {
  local tmp="$1" arch triple nats pg port_file
  read -r arch triple < <(platform)
  nats="$(fetch "https://github.com/nats-io/nats-server/releases/download/v${NATS_VERSION}/nats-server-v${NATS_VERSION}-linux-${arch}.tar.gz")"
  pg="$(fetch "https://github.com/theseus-rs/postgresql-binaries/releases/download/${POSTGRES_VERSION}/postgresql-${POSTGRES_VERSION}-${triple}.tar.gz")"

  printf '%s' "${pg}/bin/pg_ctl" >"${tmp}/pg_ctl"
  "${pg}/bin/initdb" -D "${tmp}/pg" -U postgres --auth trust -E UTF8 >"${tmp}/initdb.log" 2>&1 ||
    { cat "${tmp}/initdb.log" >&2; return 1; }
  # No TCP listener: a socket in tmp can't collide with anything else on the machine.
  "${pg}/bin/pg_ctl" -D "${tmp}/pg" -l "${tmp}/pg.log" -w \
    -o "-c listen_addresses='' -k ${tmp} -c fsync=off" start >/dev/null ||
    { cat "${tmp}/pg.log" >&2; return 1; }

  "${nats}/nats-server" -a 127.0.0.1 -p -1 -js -sd "${tmp}/js" --ports_file_dir "${tmp}" \
    -l "${tmp}/nats.log" </dev/null >/dev/null 2>&1 &
  printf '%s' "$!" >"${tmp}/nats.pid"
  port_file="${tmp}/nats-server_$(cat "${tmp}/nats.pid").ports"
  # Up to 30s on a busy machine; stop waiting as soon as the server is gone.
  for _ in $(seq 300); do
    [[ -s "${port_file}" ]] || ! kill -0 "$(cat "${tmp}/nats.pid")" 2>/dev/null && break
    sleep 0.1
  done
  [[ -s "${port_file}" ]] || { printf 'services: nats-server did not start\n' >&2; cat "${tmp}/nats.log" >&2 || true; return 1; }

  printf 'DATABASE_URL=postgres://postgres@localhost/postgres?host=%s\n' "${tmp}"
  printf 'RANGELIGHT_TESTKIT_NATS_URL=%s\n' "$(grep -o 'nats://[^"]*' "${port_file}" | head -1)"
}

# stop <tmp>: stop what start started there, and remove it.
stop() {
  local tmp="$1"
  if [[ -f "${tmp}/nats.pid" ]]; then kill "$(cat "${tmp}/nats.pid")" 2>/dev/null || true; fi
  if [[ -f "${tmp}/pg/postmaster.pid" ]]; then
    "$(cat "${tmp}/pg_ctl")" -D "${tmp}/pg" -m immediate stop >/dev/null || true
  fi
  rm -rf "${tmp}"
}

# run <command...>: the command, with DATABASE_URL and RANGELIGHT_TESTKIT_NATS_URL set.
run() {
  [[ $# -gt 0 ]] || { usage; exit 2; }
  local tmp line
  tmp="$(mktemp -d /tmp/rl-services.XXXXXX)"
  # shellcheck disable=SC2064 # expand now: tmp is local
  trap "stop '${tmp}'" EXIT
  while IFS= read -r line; do export "${line?}"; done < <(start "${tmp}")
  [[ -n "${RANGELIGHT_TESTKIT_NATS_URL:-}" ]] || { printf 'services: failed to start\n' >&2; return 1; }
  "$@"
}

usage() {
  printf 'usage: scripts/test/services.sh run <command...>\n' >&2
}

main() {
  local command="${1:-}"
  if [[ "${command}" != "run" ]] || ! declare -f "${command}" >/dev/null; then
    usage
    exit 2
  fi
  shift
  "${command}" "$@"
}

main "$@"
