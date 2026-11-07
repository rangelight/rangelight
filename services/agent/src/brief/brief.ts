import { readFileSync } from "node:fs";
import { z } from "zod";

const fields = z
  .object({
    key: z.string().regex(/^[A-Z][A-Z0-9]*-[1-9][0-9]*$/, "a ticket key like RAN-21"),
    title: z.string().min(1).max(300),
    description: z.string().max(20_000),
    criteria: z.array(z.string().min(1).max(2_000)).min(1).max(50),
  })
  .strict();

/** The ticket a lane works on, as the runner mounts it. */
export type Ticket = z.infer<typeof fields>;

// The brief reaches claude as one argv argument, and Linux caps one at 128 KiB (MAX_ARG_STRLEN).
const MAX_BRIEF_BYTES = 100_000;

const ticketSchema = fields.refine(
  (t) => Buffer.byteLength(resumeBrief(t)) <= MAX_BRIEF_BYTES,
  `the brief must stay under ${String(MAX_BRIEF_BYTES)} bytes of UTF-8; shorten the ticket`,
);

/**
 * Reads and validates the mounted ticket file. Throws on a missing or invalid file, or on a
 * ticket whose brief is too long to start claude with.
 */
export function readTicket(path: string): Ticket {
  return ticketSchema.parse(JSON.parse(readFileSync(path, "utf8")));
}

/**
 * The prompt that starts a lane: the ticket, its criteria as AC1..ACn, and the two rules
 * Rangelight's own checks enforce (ADR 0045 §3): a test named AC<n> per criterion, committed and
 * pushed.
 */
export function brief(ticket: Ticket): string {
  const criteria = ticket.criteria.map((c, i) => `AC${String(i + 1)}. ${c}`);
  const names = ticket.criteria.map((_, i) => `AC${String(i + 1)}`).join(", ");
  return [
    `You are working on ${ticket.key}: ${ticket.title}`,
    "",
    ticket.description,
    "",
    "Acceptance criteria:",
    ...criteria,
    "",
    "How Rangelight checks your work:",
    `- For each criterion add a test whose name contains its id (${names}). A criterion counts as`,
    "  proven only by such a test, new on this branch, passing in Rangelight's own run.",
    "- Commit your work on the current branch and push it with `git push`. Only what is committed",
    "  and pushed is checked.",
  ].join("\n");
}

/** The prompt that resumes a reopened lane's session: the brief again, as the digest. */
export function resumeBrief(ticket: Ticket): string {
  return `This lane was reopened. Continue where you left off.\n\n${brief(ticket)}`;
}
