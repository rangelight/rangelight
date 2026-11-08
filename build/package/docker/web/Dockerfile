# The image of the Workbench (services/web): its static build, served by unprivileged nginx
# (ADR 0035). The api and the Workbench share an origin: the ingress routes the api's paths to the
# api and everything else here (ADR 0040 §4).
# Built from the repo root: docker build -f build/package/docker/web/Dockerfile .
# Bases are pinned by digest; nginx runs as the base image's uid 101 (USER 101) on port 8080.

FROM docker.io/library/node:24.21.0-bookworm-slim@sha256:d6aa754f16b3197301076f047b5def2f02ea1dbbc2ca920407d46d7ec7f87b20 AS build
WORKDIR /src
COPY package.json pnpm-lock.yaml pnpm-workspace.yaml ./
COPY services/web/package.json services/web/
# Corepack installs the pnpm release package.json pins, verified by its sha512. The root project
# (".") brings @bufbuild/protobuf, which the generated code in shared/ts/gen imports.
RUN corepack enable && pnpm install --frozen-lockfile --filter web --filter .
COPY tsconfig.base.json ./
COPY shared/ts/gen shared/ts/gen
COPY services/web services/web
RUN pnpm --filter web run build

FROM docker.io/nginxinc/nginx-unprivileged:1.30.6-alpine-slim@sha256:5d8bb9437d74a600ec5dbf90c035451c724dc33eac8d3a71078f6f8612c345f6
COPY build/package/docker/web/nginx.conf /etc/nginx/conf.d/default.conf
COPY --from=build /src/services/web/dist /usr/share/nginx/html
EXPOSE 8080
