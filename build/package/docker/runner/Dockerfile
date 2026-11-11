# The runner (services/runner): one static Go binary on distroless, as non-root.
# Built from the repo root: docker build -f build/package/docker/runner/Dockerfile .
# Bases are pinned by digest (the multi-arch index).

FROM docker.io/library/golang:1.27.4-bookworm@sha256:4fa7c968ba93f1635670522dbb01a2c78150443e1385746924566079f36570ea AS build
WORKDIR /src
COPY go.mod go.sum ./
RUN go mod download
COPY configs configs
COPY shared/go shared/go
COPY services/runner services/runner
RUN CGO_ENABLED=0 go build -trimpath -ldflags="-s -w" -o /out/runner ./services/runner

FROM gcr.io/distroless/static-debian12:nonroot@sha256:afa5c872c891853ca7fcf1f12c3edb23f7eeef36189728842dd51042ff57f7ab
COPY --from=build /out/runner /usr/local/bin/runner
# distroless's nonroot user, by number so Kubernetes can check runAsNonRoot.
USER 65532:65532
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/runner"]
