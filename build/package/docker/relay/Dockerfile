# The image of the relay: the outbox to NATS (ADR 0035).
# Built from the repo root: docker build -f build/package/docker/relay/Dockerfile .
# Bases are pinned by digest; the runtime is distroless and runs as non-root.

FROM docker.io/library/rust:1.99.1-slim-bookworm@sha256:f2c004e3d2d8b547e8cf7ffbc29205385f4e71d23e53e4e383065db5e9f822cd AS build
# rl-proto's build script compiles api/proto with buf (the version scripts/check.sh pins).
ADD --checksum=sha256:fad1e38d6909bbb583a4da81d8c02a01046e2cd663bde7e337dd24277393764b --chmod=755 \
  https://github.com/bufbuild/buf/releases/download/v1.75.0/buf-Linux-x86_64 /usr/local/bin/buf
WORKDIR /src
COPY . .
# SQL is checked against the committed .sqlx/, not a database.
ENV SQLX_OFFLINE=true
RUN cargo build --release --locked -p relay --bin relay

FROM gcr.io/distroless/cc-debian12:nonroot@sha256:9dac0a79194e45a7da0158a9c6da57b217585af0786db3845d1f0ec1a0dd182f
COPY --from=build /src/target/release/relay /usr/local/bin/relay
# distroless's nonroot user, by number so Kubernetes can check runAsNonRoot.
USER 65532:65532
ENTRYPOINT ["/usr/local/bin/relay"]
