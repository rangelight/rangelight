import { readFileSync } from "node:fs";
import { expect, test } from "vitest";
import { ptyIn, ptyOut } from "./subjects.ts";

const readme = readFileSync(new URL("../../../api/README.md", import.meta.url), "utf8");

test("subjects: every agent subject is in api/README.md's subject map", () => {
  for (const subject of [ptyIn("<org>", "<session>"), ptyOut("<org>", "<session>")]) {
    expect(readme).toContain(`| \`${subject}\` |`);
  }
});
