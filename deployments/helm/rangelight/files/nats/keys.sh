#!/bin/sh
# Creates the NATS sign-in keys once (ADR 0021 amendment), run by the chart's pre-install and pre-upgrade
# hook Job in the nats-box image (nk, curl, jq):
# - rangelight-api-nats: the callout issuer's seed and the api's auth-user seed, mounted by the api;
# - rangelight-nats-auth: their public keys, which the bundled server's auth.conf reads.
# An existing rangelight-api-nats (or one another run creates meanwhile) is kept and its public keys derived from
# its stored seeds, so the pair always matches.
# Rotation: delete both Secrets and upgrade. The Secrets' labels are labels.json, beside this script.
set -eu

LABELS="$(cat "$(dirname "$0")/labels.json")"
secrets="https://kubernetes.default.svc/api/v1/namespaces/${NAMESPACE}/secrets"
sa=/var/run/secrets/kubernetes.io/serviceaccount
cd /tmp
# Seeds live only in these files, never in arguments or the environment, and go when the script ends.
trap 'rm -f issuer.nk auth-user.nk issuer.pub auth-user.pub out data.json data.next secret.json' EXIT

# call <method> <url> [file]: the HTTP status; the response body goes to ./out.
call() {
  curl -sS -o out -w '%{http_code}' --cacert "${sa}/ca.crt" -H "Authorization: Bearer $(cat "${sa}/token")" \
    -H 'Content-Type: application/json' -X "$1" "$2" ${3:+--data-binary "@$3"}
}

# secret <name> <key=file>...: the Secret's JSON in ./secret.json, each file's content under its key.
secret() {
  name="$1"
  shift
  printf '{}' >data.json
  for pair in "$@"; do
    jq -c --arg k "${pair%%=*}" --rawfile v "${pair#*=}" '. + {($k): ($v | @base64)}' data.json >data.next
    mv data.next data.json
  done
  jq -n --arg name "${name}" --argjson labels "${LABELS}" --slurpfile data data.json \
    '{apiVersion: "v1", kind: "Secret", type: "Opaque", metadata: {name: $name, labels: $labels}, data: $data[0]}' \
    >secret.json
}

# create <name> <key=file>...: creates the Secret; sets result to created, or to kept when it exists already.
create() {
  secret "$@"
  status="$(call POST "${secrets}" secret.json)"
  case "${status}" in
    201) result=created ;;
    409) result=kept ;;
    *) printf 'creating %s: HTTP %s\n' "$1" "${status}" >&2; cat out >&2; exit 1 ;;
  esac
  printf '%s %s\n' "${result}" "$1"
}

# stored_seeds: the seeds rangelight-api-nats holds, into issuer.nk and auth-user.nk.
stored_seeds() {
  status="$(call GET "${secrets}/rangelight-api-nats")"
  [ "${status}" = 200 ] || { printf 'reading rangelight-api-nats: HTTP %s\n' "${status}" >&2; cat out >&2; exit 1; }
  jq -r '.data["issuer-seed"]' out | base64 -d | tr -d '\n' >issuer.nk
  jq -r '.data["auth-user-seed"]' out | base64 -d | tr -d '\n' >auth-user.nk
}

# The api's seeds: new ones, unless the Secret exists, or another run created it meanwhile (409); then the
# stored ones, so the public keys below are always the pair the api holds.
nk -gen account | tr -d '\n' >issuer.nk
nk -gen user | tr -d '\n' >auth-user.nk
create rangelight-api-nats issuer-seed=issuer.nk auth-user-seed=auth-user.nk
[ "${result}" = created ] || stored_seeds
# Public keys without a line break: the server reads them as values.
nk -inkey issuer.nk -pubout | tr -d '\n' >issuer.pub
nk -inkey auth-user.nk -pubout | tr -d '\n' >auth-user.pub
create rangelight-nats-auth issuer=issuer.pub auth-user=auth-user.pub

# A kept rangelight-nats-auth must hold this pair's public keys.
status="$(call GET "${secrets}/rangelight-nats-auth")"
[ "${status}" = 200 ] || { printf 'reading rangelight-nats-auth: HTTP %s\n' "${status}" >&2; exit 1; }
for key in issuer auth-user; do
  [ "$(jq -r --arg k "${key}" '.data[$k]' out | base64 -d)" = "$(cat "${key}.pub")" ] ||
    { printf 'rangelight-nats-auth holds another %s key: delete both Secrets to rotate\n' "${key}" >&2; exit 1; }
done
