# The git mirror (ADR 0011): stock nginx, fcgiwrap and git, plus our config, CGI program and hooks.
# No maintained stock image ships the three together, so this is the smallest one that does.
# Alpine packages are pinned to the versions of the pinned base's release branch.
FROM docker.io/library/alpine:3.24.4@sha256:93b41ef40b1677fce8fcc64ff4af09daddfacf5040afde618bc46c1e2d1cc7d4

RUN apk add --no-cache \
      bash=5.3.9-r1 \
      curl=8.23.0-r0 \
      fcgiwrap=1.1.0-r9 \
      git=2.54.0-r0 \
      git-daemon=2.54.0-r0 \
      nginx=1.30.6-r0 \
    && addgroup -S -g 10001 mirror \
    && adduser -S -D -H -u 10001 -G mirror mirror

COPY services/git-mirror/nginx.conf /etc/git-mirror/nginx.conf
COPY services/git-mirror/backend.sh services/git-mirror/identity.sh services/git-mirror/sync.sh /opt/git-mirror/
COPY services/git-mirror/hooks/ /opt/git-mirror/hooks/

USER 10001:10001
EXPOSE 8080
# nginx needs the resolver as an address: take the pod's first nameserver. `-e stderr`: the root
# filesystem is read-only, so nginx's built-in error log path can't be opened before the config is read.
CMD ["sh", "-c", "awk '/^nameserver/ { ip = $2; if (ip ~ /:/) ip = \"[\" ip \"]\"; print \"resolver \" ip \" valid=30s;\"; exit }' /etc/resolv.conf >/tmp/resolver.conf && exec nginx -e stderr -c /etc/git-mirror/nginx.conf -g 'daemon off;'"]
