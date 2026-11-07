import { getRandomValues } from "node:crypto";

/** The current time. A seam so tests can fix it (ADR 0049 §2.2). */
export interface Clock {
  now(): Date;
}

/** New message ids (UUIDv7). A seam so tests can fix them (ADR 0049 §2.2). */
export interface IdGen {
  next(): string;
}

/** The system clock. */
export const systemClock: Clock = { now: () => new Date() };

/**
 * A UUIDv7 (RFC 9562 §5.7) from a millisecond timestamp and 10 random bytes. Ids made later sort
 * after earlier ones at millisecond resolution.
 */
export function uuidv7(ms: number, random: Uint8Array): string {
  const bytes = new Uint8Array(16);
  let t = ms;
  for (let i = 5; i >= 0; i--) {
    bytes[i] = t % 256;
    t = Math.floor(t / 256);
  }
  bytes.set(random.subarray(0, 10), 6);
  bytes[6] = 0x70 | ((random[0] ?? 0) & 0x0f);
  bytes[8] = 0x80 | ((random[2] ?? 0) & 0x3f);
  const hex = Buffer.from(bytes).toString("hex");
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}

/** UUIDv7 ids from the given clock and the system's random source. */
export function uuidv7Ids(clock: Clock): IdGen {
  return { next: () => uuidv7(clock.now().getTime(), getRandomValues(new Uint8Array(10))) };
}
