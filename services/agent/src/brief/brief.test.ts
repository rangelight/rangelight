import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, onTestFinished, test } from "vitest";
import { brief, readTicket, resumeBrief, type Ticket } from "./brief.ts";

const ticket: Ticket = {
  key: "MEG-901",
  title: "Expired discount codes are rejected",
  description: "Checkout accepts expired codes.",
  criteria: ["expired codes return 422", "the message names the code", "valid codes still work"],
};

test("AC2 the brief names tests AC1-AC3 and says to commit and push", () => {
  const text = brief(ticket);
  expect(text).toContain("AC1. expired codes return 422");
  expect(text).toContain("AC3. valid codes still work");
  expect(text).toContain("a test whose name contains its id (AC1, AC2, AC3)");
  expect(text).toMatch(/Commit your work[\s\S]*`git push`/);
  expect(resumeBrief(ticket)).toContain(text);
});

test("the mounted ticket file is validated", () => {
  const dir = mkdtempSync(join(tmpdir(), "rl-agent-ticket-"));
  onTestFinished(() => {
    rmSync(dir, { recursive: true, force: true });
  });
  const file = join(dir, "ticket.json");
  writeFileSync(file, JSON.stringify(ticket));
  expect(readTicket(file)).toEqual(ticket);
  writeFileSync(file, JSON.stringify({ ...ticket, criteria: [] }));
  expect(() => readTicket(file)).toThrow(/criteria/);
});

test("a ticket whose brief wouldn't fit in one argv argument is refused", () => {
  const dir = mkdtempSync(join(tmpdir(), "rl-agent-ticket-"));
  onTestFinished(() => {
    rmSync(dir, { recursive: true, force: true });
  });
  const file = join(dir, "ticket.json");
  // Within every field's character limit, but 3 bytes per character in UTF-8.
  const big = { ...ticket, criteria: Array.from({ length: 50 }, () => "€".repeat(2_000)) };
  writeFileSync(file, JSON.stringify(big));
  expect(() => readTicket(file)).toThrow(/100000 bytes/);
});
