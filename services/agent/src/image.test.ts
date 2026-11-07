// Static checks of the image definition; the image itself is built by .github/workflows/images.yaml.
import { readFileSync } from "node:fs";
import { expect, test } from "vitest";

const dockerfile = readFileSync(
  new URL("../../../build/package/docker/agent/Dockerfile", import.meta.url),
  "utf8",
);

test("the agent runs under tini, so zombies are reaped and SIGTERM reaches node", () => {
  expect(dockerfile).toMatch(/^ENTRYPOINT \["\/usr\/local\/bin\/tini", "--", "node", /m);
});

test("every downloaded binary is pinned by its sha256", () => {
  const adds = dockerfile.match(/^ADD .*/gm) ?? [];
  expect(adds.length).toBeGreaterThanOrEqual(4);
  for (const add of adds) expect(add).toMatch(/--checksum=sha256:[0-9a-f]{64} /);
});
