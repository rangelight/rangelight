// What the specs share: the sessions they start from, and the stub's controls (test/e2e/stub.ts).
import AxeBuilder from "@axe-core/playwright";
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
  watchingAll: z.number(),
  signedOut: z.boolean(),
  signIns: z.array(z.string()),
  lastResize: z.object({ cols: z.number(), rows: z.number() }).nullable(),
  input: z.string(),
});
export const stubState = async (page: Page) => StubState.parse(await stub(page, "state"));

export const row = (page: Page, key: string) => page.getByRole("row").filter({ hasText: key });

/** No horizontal page scroll, and nothing reaches past the screen's edge but the key bar's keys,
 * which scroll in their own strip. */
export async function fitsTheWidth(page: Page): Promise<void> {
  const wide = await page.evaluate(() => {
    const scroller = document.scrollingElement;
    if (scroller && scroller.scrollWidth > innerWidth)
      return [`page ${String(scroller.scrollWidth)}`];
    return [...document.querySelectorAll("body *")]
      .filter((el) => !el.closest('[aria-label="Terminal keys"]'))
      .filter((el) => el.getBoundingClientRect().right > innerWidth + 0.5)
      .map((el) => el.outerHTML.slice(0, 80));
  });
  expect(wide).toEqual([]);
}

/** Every link, button and tab on screen is at least 44 by 44 px (a terminal key 40 by 44). */
export async function touchTargets(page: Page): Promise<void> {
  const small = await page.evaluate(() =>
    [...document.querySelectorAll("a, button, [role=tab]")]
      .map((el) => ({ el, r: el.getBoundingClientRect() }))
      .filter(({ r }) => r.width > 0 && r.height > 0 && r.right > 0 && r.left < innerWidth)
      // The key bar's keys may be 40 px wide, to fit more of them.
      .filter(({ el, r }) => {
        const minWidth = el.closest('[aria-label="Terminal keys"]') ? 39.5 : 43.5;
        return r.width < minWidth || r.height < 43.5;
      })
      .map(({ el, r }) => `${el.textContent} ${String(r.width)}x${String(r.height)}`),
  );
  expect(small).toEqual([]);
}

/** No serious or critical axe violation. */
export async function axe(page: Page): Promise<void> {
  const { violations } = await new AxeBuilder({ page }).analyze();
  expect(
    violations.filter((v) => v.impact === "serious" || v.impact === "critical").map((v) => v.id),
  ).toEqual([]);
}
