#!/usr/bin/env bash
# Installs every tool pinned in scripts/tools.env into ${XDG_CACHE_HOME:-~/.cache}/rangelight/tools/<name>-<version>/,
# the same way locally and in CI: release archives verified against their sha256 before they are unpacked, Go and
# Rust tools built from their registries' checksummed sources, Python tools from hash-checked wheels. No sudo, no
# system packages. A tool already in the cache is left alone, so a second run downloads nothing.
# Usage: scripts/tools.sh [install]   install the pinned set (the default)
#        scripts/tools.sh path        print the PATH entries of the pinned set
set -euo pipefail
shopt -s inherit_errexit
export LC_ALL=C

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CACHE="${XDG_CACHE_HOME:-${HOME}/.cache}/rangelight/tools"
# shellcheck source=scripts/tools.env
source "${ROOT}/scripts/tools.env"

# key_of <tool>: its tools.env key (gitleaks -> GITLEAKS, cargo-nextest -> CARGO_NEXTEST); version_of: its version.
key_of() { local k="${1^^}"; printf '%s' "${k//-/_}"; }
version_of() { local k; k="$(key_of "$1")"; printf '%s' "${!k:?$1 is not pinned in scripts/tools.env}"; }
dir_of() { printf '%s/%s-%s' "${CACHE}" "$1" "$(version_of "$1")"; }

path() {
  local tool entries=()
  for tool in ${TOOLS}; do entries+=("$(dir_of "${tool}")/bin"); done
  (IFS=:; printf '%s\n' "${entries[*]}")
}

# platform: os_arch as tools.env keys it, or fail.
platform() {
  case "$(uname -s)-$(uname -m)" in
    Linux-x86_64) printf 'linux_amd64' ;;
    Linux-aarch64 | Linux-arm64) printf 'linux_arm64' ;;
    *) printf 'tools: nothing is pinned for %s\n' "$(uname -sm)" >&2; return 1 ;;
  esac
}

# release <tool> <arch>: the archive's URL, then the path in it to install (a binary, or a whole tree).
release() {
  local v gh="https://github.com" a="$2" x t
  v="$(version_of "$1")"
  x="$([[ "${a}" == amd64 ]] && printf x86_64 || printf aarch64)"
  t="${x}-unknown-linux"
  case "$1" in
    gitleaks) printf '%s %s\n' "${gh}/gitleaks/gitleaks/releases/download/v${v}/gitleaks_${v}_linux_${a/amd64/x64}.tar.gz" gitleaks ;;
    buf) printf '%s %s\n' "${gh}/bufbuild/buf/releases/download/v${v}/buf-Linux-${x}.tar.gz" buf/bin/buf ;;
    shellcheck) printf '%s %s\n' "${gh}/koalaman/shellcheck/releases/download/v${v}/shellcheck-v${v}.linux.${x}.tar.gz" "shellcheck-v${v}/shellcheck" ;;
    helm) printf '%s %s\n' "https://get.helm.sh/helm-v${v}-linux-${a}.tar.gz" "linux-${a}/helm" ;;
    kubeconform) printf '%s %s\n' "${gh}/yannh/kubeconform/releases/download/v${v}/kubeconform-linux-${a}.tar.gz" kubeconform ;;
    golangci-lint) printf '%s %s\n' "${gh}/golangci/golangci-lint/releases/download/v${v}/golangci-lint-${v}-linux-${a}.tar.gz" "golangci-lint-${v}-linux-${a}/golangci-lint" ;;
    cargo-nextest) printf '%s %s\n' "${gh}/nextest-rs/nextest/releases/download/cargo-nextest-${v}/cargo-nextest-${v}-${t}-gnu.tar.gz" cargo-nextest ;;
    cargo-deny) printf '%s %s\n' "${gh}/EmbarkStudios/cargo-deny/releases/download/${v}/cargo-deny-${v}-${t}-musl.tar.gz" "cargo-deny-${v}-${t}-musl/cargo-deny" ;;
    # The arm64 build is the only gnu one.
    cargo-machete)
      t="${t}-$([[ "${a}" == amd64 ]] && printf musl || printf gnu)"
      printf '%s %s\n' "${gh}/bnjbvr/cargo-machete/releases/download/v${v}/cargo-machete-v${v}-${t}.tar.gz" "cargo-machete-v${v}-${t}/cargo-machete"
      ;;
    nats-server) printf '%s %s\n' "${gh}/nats-io/nats-server/releases/download/v${v}/nats-server-v${v}-linux-${a}.tar.gz" "nats-server-v${v}-linux-${a}/nats-server" ;;
    postgresql) printf '%s %s\n' "${gh}/theseus-rs/postgresql-binaries/releases/download/${v}/postgresql-${v}-${t}-gnu.tar.gz" "postgresql-${v}-${t}-gnu" ;;
    uv) printf '%s %s\n' "${gh}/astral-sh/uv/releases/download/${v}/uv-${t}-gnu.tar.gz" "uv-${t}-gnu/uv" ;;
    *) return 1 ;;
  esac
}

# unpack_verified <url> <sha256 key> <part dir>: download it, check it against tools.env, unpack it into x/.
unpack_verified() {
  local url="$1" key="$2" part="$3" sum
  sum="${!key:?no ${key} in scripts/tools.env}"
  mkdir -p "${part}/x"
  curl -sSfL --retry 3 -o "${part}/archive" "${url}"
  printf '%s  %s\n' "${sum}" "${part}/archive" | sha256sum -c --quiet - >&2 ||
    { printf 'tools: %s does not match its sha256 in scripts/tools.env\n' "${url}" >&2; return 1; }
  tar -xzf "${part}/archive" -C "${part}/x"
}

# from_release <tool> <part dir>: download, verify, unpack; a binary goes to bin/, a tree becomes the dir.
from_release() {
  local tool="$1" part="$2" plat url member
  plat="$(platform)"
  read -r url member < <(release "${tool}" "${plat#*_}")
  unpack_verified "${url}" "$(key_of "${tool}")_SHA256_${plat}" "${part}"
  if [[ -d "${part}/x/${member}" ]]; then
    mv "${part}/x/${member}" "${part}/tree"
  else
    mkdir -p "${part}/tree/bin"
    mv "${part}/x/${member}" "${part}/tree/bin/"
  fi
}

# go_verified <command...>: go with nothing able to switch off module verification (explicit values beat `go env -w`).
go_verified() {
  env GOSUMDB=sum.golang.org GONOSUMDB= GONOSUMCHECK= GOPRIVATE= GOINSECURE= "$@"
}

# from_source <tool> <part dir>: tools without a release binary, built from verified sources.
from_source() {
  local tool="$1" part="$2" v
  v="$(version_of "${tool}")"
  case "${tool}" in
    govulncheck)
      # Built with the go.mod toolchain; the Go checksum database verifies every module.
      (cd "${ROOT}" && go_verified GOBIN="${part}/tree/bin" GOFLAGS=-mod=mod \
        GOTOOLCHAIN="$(awk '$1 == "toolchain" { print $2 }' go.mod)" go install "golang.org/x/vuln/cmd/govulncheck@v${v}")
      ;;
    dex)
      unpack_verified "https://github.com/dexidp/dex/archive/refs/tags/v${v}.tar.gz" DEX_SHA256 "${part}"
      # Its own go.mod picks the toolchain and go.sum verifies every module; the version is stamped for need.
      (cd "${part}/x/dex-${v}" && go_verified CGO_ENABLED=0 GOFLAGS=-mod=readonly go build -trimpath \
        -ldflags "-X main.version=v${v}" -o "${part}/tree/bin/dex" ./cmd/dex)
      ;;
    sqlx-cli)
      (cd "${ROOT}" && cargo install --quiet --locked --root "${part}/tree" "sqlx-cli@${v}" --no-default-features --features postgres)
      ;;
    yamllint | check-jsonschema)
      local key
      key="$(key_of "${tool}")_REQUIREMENTS"
      printf '%s' "${!key}" >"${part}/requirements.txt"
      # Only uv's own managed CPython, never one already on the machine. Relocatable, so it survives the rename;
      # only the tool itself goes on PATH, not the venv's python.
      env UV_PYTHON_INSTALL_DIR="${CACHE}/python" UV_CACHE_DIR="${part}/uv-cache" \
        "$(dir_of uv)/bin/uv" venv --quiet --no-config --managed-python --relocatable --python "${PYTHON}" "${part}/tree/venv"
      env UV_PYTHON_INSTALL_DIR="${CACHE}/python" UV_CACHE_DIR="${part}/uv-cache" VIRTUAL_ENV="${part}/tree/venv" \
        "$(dir_of uv)/bin/uv" pip install --quiet --no-config --require-hashes --only-binary :all: --no-deps \
        -r "${part}/requirements.txt"
      mkdir -p "${part}/tree/bin"
      ln -s "../venv/bin/${tool}" "${part}/tree/bin/${tool}"
      ;;
    *) return 1 ;;
  esac
}

# install_one <tool>: into its cache dir, unless it is there. Built aside, then renamed: never half a tool.
install_one() {
  local tool="$1" dir part
  dir="$(dir_of "${tool}")"
  [[ ! -d "${dir}" ]] || return 0
  printf 'tools: installing %s %s\n' "${tool}" "$(version_of "${tool}")" >&2
  part="$(mktemp -d "${dir}.part.XXXXXX")"
  # shellcheck disable=SC2064 # expand now: part is local
  trap "rm -rf '${part}'" EXIT
  if release "${tool}" amd64 >/dev/null; then from_release "${tool}" "${part}"; else from_source "${tool}" "${part}"; fi
  # -T: if another run got there first, fail rather than nest this tree inside its dir.
  mv -T "${part}/tree" "${dir}"
  rm -rf "${part}"
  trap - EXIT
}

install() {
  local tool
  mkdir -p "${CACHE}"
  for tool in ${TOOLS}; do install_one "${tool}"; done
  printf 'tools: ok (%s)\n' "${CACHE}"
}

usage() {
  printf 'usage: scripts/tools.sh [install|path]\n' >&2
}

main() {
  local command="${1:-install}"
  if [[ "${command}" != install && "${command}" != path ]] || ! declare -f "${command}" >/dev/null; then
    usage
    exit 2
  fi
  shift || true
  "${command}" "$@"
}

main "$@"
