import { expect, test } from "vitest";
import { safeHttps } from "./ticket-link";

test("only absolute https URLs are links", () => {
  expect(safeHttps("https://linear.app/x/issue/RAN-1")).toBe(true);
  for (const bad of [
    "",
    "http://linear.app/x",
    "javascript:alert(1)",
    "//evil.example",
    "https://",
  ]) {
    expect(safeHttps(bad), bad).toBe(false);
  }
});
