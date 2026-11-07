# The lane agent (services/agent): Node, git and the unmodified Claude Code binary, a pinned
# release whose checksum is verified. No secrets and no API key: the member signs in with
# Claude Code's own /login (ADR 0046). Build from the repo root.
ARG NODE_IMAGE=node:24.21.0-bookworm-slim@sha256:d6aa754f16b3197301076f047b5def2f02ea1dbbc2ca920407d46d7ec7f87b20

FROM ${NODE_IMAGE} AS deps
WORKDIR /app
COPY package.json pnpm-lock.yaml pnpm-workspace.yaml ./
COPY services/agent/package.json services/agent/
# Corepack installs the pnpm release package.json pins, verified by its sha512.
RUN corepack enable && pnpm install --frozen-lockfile --prod --filter agent

# The same binary and checksums the official installer (claude.ai/install.sh) downloads and checks,
# from downloads.claude.ai/claude-code-releases/2.1.319/manifest.json.
FROM scratch AS claude-amd64
ADD --checksum=sha256:dabafec82ca1319fa43d8be36a10fc61b1d11821cb98ad5df04ad95e199b8eed --chmod=755 \
  https://downloads.claude.ai/claude-code-releases/2.1.319/linux-x64/claude /claude
FROM scratch AS claude-arm64
ADD --checksum=sha256:a7e687fd1cb054d651c7588223de3786c99d4c682365e3c3ae1e59ce7f3a591f --chmod=755 \
  https://downloads.claude.ai/claude-code-releases/2.1.319/linux-arm64/claude /claude
FROM claude-${TARGETARCH} AS claude

# tini as PID 1: reaps the zombies of the model's commands and passes SIGTERM on to node.
FROM scratch AS tini-amd64
ADD --checksum=sha256:c5b0666b4cb676901f90dfcb37106783c5fe2077b04590973b885950611b30ee --chmod=755 \
  https://github.com/krallin/tini/releases/download/v0.19.0/tini-static-amd64 /tini
FROM scratch AS tini-arm64
ADD --checksum=sha256:eae1d3aa50c48fb23b8cbdf4e369d0910dfc538566bfd09df89a774aa84a48b9 --chmod=755 \
  https://github.com/krallin/tini/releases/download/v0.19.0/tini-static-arm64 /tini
FROM tini-${TARGETARCH} AS tini

FROM ${NODE_IMAGE}
# git for the worktree; script(1) (bsdutils), stty and sh are already in the base image.
RUN apt-get update \
  && apt-get install -y --no-install-recommends ca-certificates git \
  && rm -rf /var/lib/apt/lists/*
COPY --from=claude /claude /usr/local/bin/claude
COPY --from=tini /tini /usr/local/bin/tini
WORKDIR /app
COPY --from=deps /app/node_modules node_modules
COPY --from=deps /app/services/agent/node_modules services/agent/node_modules
COPY api/jsonschema/agent-event.v1alpha1.schema.json api/jsonschema/
COPY configs/agent.default.toml configs/
COPY services/agent/package.json services/agent/
COPY services/agent/src services/agent/src
ENV NODE_ENV=production
# uid 1000, as ADR 0045 §5 has it; the runner mounts the worktree and the Claude home.
USER node
ENTRYPOINT ["/usr/local/bin/tini", "--", "node", "services/agent/src/main.ts"]
