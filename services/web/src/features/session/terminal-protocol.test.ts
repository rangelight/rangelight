import { expect, test } from "vitest";
import { inputFrames, MAX_FRAME_BYTES, outputBytes, resizeFrame } from "./terminal-protocol";

const utf8 = (text: string) => new TextEncoder().encode(text);

const decode = (bytes: Uint8Array | undefined) => new TextDecoder().decode(bytes);

test("input round-trips as an output frame, UTF-8 intact", () => {
  const [frame] = inputFrames(utf8("/login ✓\r"));
  expect(JSON.parse(frame ?? "")).toMatchObject({ type: "input" });
  const echoed = (frame ?? "").replace('"input"', '"output"');
  expect(decode(outputBytes(echoed))).toBe("/login ✓\r");
});

test("a large paste is split into frames within the schema's cap", () => {
  const frames = inputFrames(utf8("x".repeat(MAX_FRAME_BYTES * 2 + 1)));
  expect(frames).toHaveLength(3);
  // 65,536 base64 characters plus the frame's 27 characters of JSON around them.
  for (const f of frames) expect(f.length).toBeLessThanOrEqual(65_536 + 27);
});

test("resize carries cols and rows, clamped to the schema's 1 to 1000", () => {
  expect(JSON.parse(resizeFrame(120, 32))).toEqual({ type: "resize", cols: 120, rows: 32 });
  expect(JSON.parse(resizeFrame(0, 5000))).toEqual({ type: "resize", cols: 1, rows: 1000 });
  expect(JSON.parse(resizeFrame(Number.NaN, 2.6))).toEqual({ type: "resize", cols: 1, rows: 3 });
});

test("anything but a valid output frame is dropped", () => {
  for (const bad of [
    42,
    "not json",
    '{"type":"input","data":"aGk="}',
    '{"type":"output","data":"/login\\r"}',
    '{"type":"output","data":"aGk=","extra":1}',
  ]) {
    expect(outputBytes(bad), String(bad)).toBeUndefined();
  }
});
