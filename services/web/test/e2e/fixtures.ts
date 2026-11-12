// What both specs share: the sessions they start from, and the stub's controls (test/e2e/stub.ts).
import { expect, type Page } from "@playwright/test";
import { DesiredState } from "@gen/proto/rangelight/runner/v1alpha1/runner_pb.ts";
import { z } from "zod";
import { STUB_PORT, type StubSession } from "./stub";

const STUB = `http://127.0.0.1:${String(STUB_PORT)}/__stub`;
export const FIRST = "019a5f10-0000-7000-8000-000000000101";
export const SESSIONS: StubSession[] = [
  {
    id: FIRST,
    ticketKey: "RAN-101",
    title: "Cart totals round half-even",
    repo: "acme/storefront",
    state: DesiredState.HOT,
    minutesAgo: 125,
  },
  {
    id: "019a5f10-0000-7000-8000-000000000102",
    ticketKey: "RAN-102",
    title: "Webhook retries back off exponentially",
    repo: "acme/payments",
    state: DesiredState.WARM,
    minutesAgo: 42,
  },
  {
    id: "019a5f10-0000-7000-8000-000000000103",
    ticketKey: "RAN-103",
    title: "Password reset links expire after 30 minutes",
    repo: "",
    state: DesiredState.COLD,
    minutesAgo: 3 * 24 * 60 + 65,
  },
];

export async function stub(page: Page, command: string, data: object = {}): Promise<unknown> {
  const res = await page.request.post(`${STUB}/${command}`, { data });
  expect(res.ok()).toBe(true);
  return res.json();
}

const StubState = z.object({
  watching: z.number(),
  lastResize: z.object({ cols: z.number(), rows: z.number() }).nullable(),
  input: z.string(),
});
export const stubState = async (page: Page) => StubState.parse(await stub(page, "state"));

export const row = (page: Page, key: string) => page.getByRole("row").filter({ hasText: key });
