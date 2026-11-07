import { expect, test } from "vitest";
import { cleanMessage } from "./runtime.ts";

test("a message can't press Enter or Escape or send terminal sequences", () => {
  expect(cleanMessage("a\tb\r\x1b[2Jc\nd\x03\u009b")).toBe("a b[2Jc\nd");
});
