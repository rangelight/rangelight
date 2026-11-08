#!/usr/bin/env bash
# Real Postgres, NATS and Dex for the Rust tests, without Docker or system packages (ADR 0049 §8).
# Downloads pinned, checksum-verified release binaries into a cache (Dex publishes none: its
# pinned source is built once with Go, whose go.sum verifies every module), starts them in a temp
# dir (Postgres on a Unix socket, NATS and Dex on random ports), runs the command, and tears them
# down.
# Usage: scripts/test/services.sh run <command...>
set -euo pipefail
shopt -s inherit_errexit

NATS_VERSION="2.15.2"
POSTGRES_VERSION="18.6.1"
DEX_VERSION="2.46.1"
# Where the tests' sign-ins come back to; never reached, the tests read the code off the redirect.
DEX_REDIRECT="https://rangelight.test/auth/callback"
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
    "dex-${DEX_VERSION}.tar.gz") printf '05e2e149b53f9876269362fefa9061b4b1abd3d5437f23247d79e44a77ac487d' ;;
    *) return 1 ;;
  esac
}

# fetch <url> [archive]: download into the cache once (named archive, by default the URL's last
# segment), verify, unpack; print the unpacked directory.
fetch() {
  local url="$1" archive="${2:-${1##*/}}" dir
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

# dex_binary: Dex built from its pinned, verified source, once; print its path.
dex_binary() {
  local src
  src="$(fetch "https://github.com/dexidp/dex/archive/refs/tags/v${DEX_VERSION}.tar.gz" "dex-${DEX_VERSION}.tar.gz")"
  if [[ ! -x "${src}/bin/dex" ]]; then
    (cd "${src}" && CGO_ENABLED=0 go build -mod=readonly -trimpath -o bin/dex.part ./cmd/dex) >&2
    mv "${src}/bin/dex.part" "${src}/bin/dex"
  fi
  printf '%s' "${src}/bin/dex"
}

# free_port: a TCP port nothing on 127.0.0.1 listens on.
free_port() {
  local port
  while :; do
    port="$((20000 + RANDOM % 40000))"
    (: <"/dev/tcp/127.0.0.1/${port}") 2>/dev/null || { printf '%s' "${port}"; return 0; }
  done
}

# dex_config <issuer> <port>: Dex with its password database (it sets preferred_username, which
# the mock connector doesn't) and two clients. The hash is Dex's documented example hash of
# "password". kilgore and impostor share the username Kilgore; dotted's isn't a member name.
dex_config() {
  # shellcheck disable=SC2016 # a bcrypt hash, not an expansion
  local issuer="$1" port="$2" hash='$2a$10$2b2cU8CPhOTaGrs1HRQuAueS7JTT5ZHsHSzYiFPm1leZck7Mc8T4W'
  cat <<YAML
---
issuer: "${issuer}"
storage:
  type: "memory"
web:
  http: "127.0.0.1:${port}"
logger:
  level: "warn"
oauth2:
  skipApprovalScreen: true
enablePasswordDB: true
staticPasswords:
  - email: "kilgore@example.com"
    hash: "${hash}"
    username: "kilgore"
    preferredUsername: "Kilgore"
    userID: "08a8684b-db88-4b73-90a9-3cd1661f5466"
  - email: "impostor@example.com"
    hash: "${hash}"
    username: "impostor"
    preferredUsername: "kilgore"
    userID: "6f1c3d1e-27a4-4a8e-9a1f-1b4b2c3d4e5f"
  - email: "dotted@example.com"
    hash: "${hash}"
    username: "dotted"
    preferredUsername: "kil.gore"
    userID: "9b2e4f60-5d1a-4c3b-8e7f-0a1b2c3d4e5f"
staticClients:
  - id: "rangelight-api"
    name: "Rangelight"
    secretEnv: "RL_DEX_CLIENT_SECRET"
    redirectURIs: ["${DEX_REDIRECT}"]
  - id: "rangelight-other"
    name: "Another client"
    secretEnv: "RL_DEX_CLIENT_SECRET"
    redirectURIs: ["${DEX_REDIRECT}"]
YAML
}

# start_dex <tmp>: start Dex under tmp; print the environment the tests read.
start_dex() {
  local tmp="$1" dex port="" issuer="" attempt
  dex="$(dex_binary)"
  # A throwaway client secret for this run only; the tests read it from the file.
  (umask 077 && od -An -N24 -tx1 /dev/urandom | tr -d ' \n' >"${tmp}/dex-client-secret")
  # A free port can be taken before Dex binds it: try another, up to three times.
  for attempt in 1 2 3; do
    port="$(free_port)"
    issuer="http://127.0.0.1:${port}/dex"
    dex_config "${issuer}" "${port}" >"${tmp}/dex.yaml"
    RL_DEX_CLIENT_SECRET="$(cat "${tmp}/dex-client-secret")" "${dex}" serve "${tmp}/dex.yaml" \
      </dev/null >"${tmp}/dex.log" 2>&1 &
    printf '%s' "$!" >"${tmp}/dex.pid"
    for _ in $(seq 300); do
      curl -sf -o /dev/null "${issuer}/.well-known/openid-configuration" || ! kill -0 "$(cat "${tmp}/dex.pid")" 2>/dev/null && break
      sleep 0.1
    done
    curl -sf -o /dev/null "${issuer}/.well-known/openid-configuration" && break
    kill "$(cat "${tmp}/dex.pid")" 2>/dev/null || true
    printf 'services: dex did not start (attempt %s)\n' "${attempt}" >&2
    cat "${tmp}/dex.log" >&2 || true
    [[ "${attempt}" -lt 3 ]] || return 1
  done
  printf 'RANGELIGHT_TESTKIT_DEX_ISSUER=%s\n' "${issuer}"
  printf 'RANGELIGHT_TESTKIT_DEX_SECRET=%s\n' "${tmp}/dex-client-secret"
  printf 'RANGELIGHT_TESTKIT_DEX_REDIRECT=%s\n' "${DEX_REDIRECT}"
}

# start <tmp>: start the servers under tmp; print the environment the tests read.
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
  start_dex "${tmp}"
}

# stop <tmp>: stop what start started there, and remove it.
stop() {
  local tmp="$1"
  if [[ -f "${tmp}/nats.pid" ]]; then kill "$(cat "${tmp}/nats.pid")" 2>/dev/null || true; fi
  if [[ -f "${tmp}/dex.pid" ]]; then kill "$(cat "${tmp}/dex.pid")" 2>/dev/null || true; fi
  if [[ -f "${tmp}/pg/postmaster.pid" ]]; then
    "$(cat "${tmp}/pg_ctl")" -D "${tmp}/pg" -m immediate stop >/dev/null || true
  fi
  rm -rf "${tmp}"
}

# run <command...>: the command, with DATABASE_URL and the RANGELIGHT_TESTKIT_* variables set.
run() {
  [[ $# -gt 0 ]] || { usage; exit 2; }
  local tmp line
  tmp="$(mktemp -d /tmp/rl-services.XXXXXX)"
  # shellcheck disable=SC2064 # expand now: tmp is local
  trap "stop '${tmp}'" EXIT
  while IFS= read -r line; do export "${line?}"; done < <(start "${tmp}")
  [[ -n "${RANGELIGHT_TESTKIT_DEX_ISSUER:-}" ]] || { printf 'services: failed to start\n' >&2; return 1; }
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
