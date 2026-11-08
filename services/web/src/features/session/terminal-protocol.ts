// Frames of api/jsonschema/terminal.v1alpha1.schema.json. Types come from the schema (shared/ts/gen);
// what arrives from the api is checked with zod before a byte reaches the terminal.
import type {
  TerminalInput,
  TerminalOutput,
  TerminalResize,
} from "@gen/terminal.v1alpha1.schema.d.ts";
import { z } from "zod";

/** The schema's cap: 65536 base64 characters, 48 KiB of bytes. */
export const MAX_FRAME_BYTES = 49_152;

const Output = z.strictObject({
  type: z.literal("output"),
  data: z.base64().min(4).max(65_536),
}) satisfies z.ZodType<TerminalOutput>;

function toBase64(bytes: Uint8Array): string {
  let bin = "";
  for (const b of bytes) bin += String.fromCharCode(b);
  return btoa(bin);
}

const fromBase64 = (data: string): Uint8Array =>
  Uint8Array.from(atob(data), (c) => c.charCodeAt(0));

/** Bytes for the PTY, as input frames of at most MAX_FRAME_BYTES each. */
export function inputFrames(bytes: Uint8Array): string[] {
  const frames: string[] = [];
  for (let i = 0; i < bytes.length; i += MAX_FRAME_BYTES) {
    const frame: TerminalInput = {
      type: "input",
      data: toBase64(bytes.subarray(i, i + MAX_FRAME_BYTES)),
    };
    frames.push(JSON.stringify(frame));
  }
  return frames;
}

/** The schema's bounds for a terminal's size in cells. */
const cells = (n: number): number => Math.min(1000, Math.max(1, Math.round(n) || 1));

export function resizeFrame(cols: number, rows: number): string {
  const frame: TerminalResize = { type: "resize", cols: cells(cols), rows: cells(rows) };
  return JSON.stringify(frame);
}

/** The bytes of an output frame, or undefined for anything that isn't one. */
export function outputBytes(frame: unknown): Uint8Array | undefined {
  if (typeof frame !== "string") return undefined;
  let json: unknown;
  try {
    json = JSON.parse(frame);
  } catch {
    return undefined;
  }
  const parsed = Output.safeParse(json);
  return parsed.success ? fromBase64(parsed.data.data) : undefined;
}
