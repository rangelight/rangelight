import { expect, test } from "vitest";
import { elapsed } from "./time";

test("elapsed picks the two largest units", () => {
  const t0 = 1_000_000_000;
  expect(elapsed(t0, t0 + 45_000)).toBe("45s");
  expect(elapsed(t0, t0 + 12 * 60_000)).toBe("12m");
  expect(elapsed(t0, t0 + (2 * 60 + 5) * 60_000)).toBe("2h 05m");
  expect(elapsed(t0, t0 + (3 * 24 + 4) * 3_600_000)).toBe("3d 4h");
  expect(elapsed(t0, t0 - 5_000)).toBe("0s");
});
