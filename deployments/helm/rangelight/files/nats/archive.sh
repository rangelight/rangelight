#!/bin/sh
# Archives the JetStream streams an install from before sign-in left in the global account ($G), before the NATS
# server starts (RAN-78). Since sign-in, JetStream is RL's, so the server never recovers $G's streams, and its full
# /healthz (the startup probe) fails on them at every start. They move, whole, beside the store on the same volume.
# It acts only when this server's JetStream is RL's and $G holds nothing but Rangelight's streams (rl-<org>-*);
# anything else is logged and left. It never touches RL, and never overwrites an archive.
# Usage: archive.sh <store dir> <auth.conf>
set -eu

store="$1"
global="${store}/\$G"
log() { printf 'nats-archive: %s\n' "$*"; }

if [ ! -d "${global}" ]; then
  log "no \$G in ${store}: nothing to archive"
  exit 0
fi
# RL's own dir appears only with its first stream, after the server is up; the config names it from the start.
if [ ! -d "${store}/RL" ] && ! grep -Eq '^[[:space:]]*RL[[:space:]]*\{' "$2"; then
  log "this server's JetStream isn't RL's: leaving ${global}"
  exit 0
fi
for entry in "${global}"/* "${global}"/.[!.]*; do
  [ -e "${entry}" ] || continue
  if [ "${entry##*/}" != "streams" ]; then
    log "${global} holds ${entry##*/}, not only streams: leaving it"
    exit 0
  fi
done
streams=""
for stream in "${global}/streams"/* "${global}/streams"/.[!.]*; do
  [ -e "${stream}" ] || continue
  case "${stream##*/}" in
    rl-?*-?*) streams="${streams} ${stream##*/}" ;;
    *)
      log "${global} holds the stream ${stream##*/}, which isn't Rangelight's: leaving it"
      exit 0
      ;;
  esac
done
if [ -z "${streams}" ]; then
  log "${global} holds no stream: leaving it"
  exit 0
fi
archive="${store%/*}/archived-G-$(date -u +%Y%m%dT%H%M%SZ)"
if [ -e "${archive}" ]; then
  log "${archive} exists: leaving ${global}"
  exit 0
fi
mv "${global}" "${archive}"
log "archived the streams${streams} from ${global} to ${archive}"
