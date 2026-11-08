import AxeBuilder from "@axe-core/playwright";
import { expect, type Page, test } from "@playwright/test";
import { DesiredState, Phase } from "@gen/proto/rangelight/runner/v1alpha1/runner_pb.ts";
import { z } from "zod";
import { STUB_PORT, type StubSession } from "./stub";

const STUB = `http://127.0.0.1:${String(STUB_PORT)}/__stub`;
const FIRST = "019a5f10-0000-7000-8000-000000000101";
const SESSIONS: StubSession[] = [
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

async function stub(page: Page, command: string, data: object = {}): Promise<unknown> {
  const res = await page.request.post(`${STUB}/${command}`, { data });
  expect(res.ok()).toBe(true);
  return res.json();
}

const StubState = z.object({
  watching: z.number(),
  lastResize: z.object({ cols: z.number(), rows: z.number() }).nullable(),
});
const stubState = async (page: Page) => StubState.parse(await stub(page, "state"));

const row = (page: Page, key: string) => page.getByRole("row").filter({ hasText: key });

test.beforeEach(async ({ page }) => {
  await stub(page, "reset", { sessions: SESSIONS });
});

test("AC1 every session is a row: ticket and Linear link, repo, state, age", async ({ page }) => {
  await page.goto("/");
  const r = row(page, "RAN-101");
  await expect(r.getByRole("link", { name: /RAN-101/ })).toHaveAttribute(
    "href",
    "https://linear.app/example/issue/RAN-101",
  );
  await expect(r.getByRole("link", { name: /RAN-101/ })).toHaveAttribute("target", "_blank");
  await expect(r.getByRole("link", { name: "Cart totals round half-even" })).toHaveAttribute(
    "href",
    `/sessions/${FIRST}`,
  );
  await expect(r).toContainText("acme/storefront");
  await expect(r).toContainText("Hot");
  await expect(r).toContainText("2h 05m");
  await expect(row(page, "RAN-103")).toContainText("no repo yet");
  await expect(row(page, "RAN-103")).toContainText("3d 1h");
});

test("AC1 rows update live from one WatchSessions stream", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByText("Live")).toBeVisible();
  await expect.poll(async () => (await stubState(page)).watching).toBe(1);
  await stub(page, "emit", { sessionId: FIRST, state: DesiredState.WARM });
  await expect(row(page, "RAN-101")).toContainText("Warm");
});

test("AC1 the phase follows the runner's reports, on the list and in the feed", async ({
  page,
}) => {
  await page.goto("/");
  await expect(row(page, "RAN-101")).toContainText("Not reported");
  await expect.poll(async () => (await stubState(page)).watching).toBe(1);
  await stub(page, "report", { sessionId: FIRST, phase: Phase.RUNNING });
  await expect(row(page, "RAN-101")).toContainText("Running");

  await page.goto(`/sessions/${FIRST}`);
  await expect(page.getByRole("heading", { name: "Cart totals round half-even" })).toBeVisible();
  await expect.poll(async () => (await stubState(page)).watching).toBe(1);
  await stub(page, "report", {
    sessionId: FIRST,
    phase: Phase.FAILED,
    message: "image pull failed",
  });
  await expect(page.getByRole("region", { name: "Feed" })).toContainText(
    "The runner reports failed: image pull failed",
  );
  await expect(page.locator("header")).toContainText("Failed");
});

test("AC1 a session created after the page loaded shows up without a reload", async ({ page }) => {
  await page.goto("/");
  await expect(row(page, "RAN-101")).toBeVisible();
  await expect.poll(async () => (await stubState(page)).watching).toBe(1);
  await stub(page, "create", {
    id: "019a5f10-0000-7000-8000-000000000104",
    ticketKey: "RAN-104",
    title: "Invoices list paginates past 1000 rows",
    repo: "",
    state: DesiredState.HOT,
    minutesAgo: 0,
  });
  const added = row(page, "RAN-104");
  await expect(added).toContainText("Invoices list paginates past 1000 rows");
  await expect(added.getByRole("link", { name: /RAN-104/ })).toHaveAttribute(
    "href",
    "https://linear.app/example/issue/RAN-104",
  );
  await expect(page.getByRole("row")).toHaveCount(5);
});

test("AC1 missed events offer a reconnect that reloads the list", async ({ page }) => {
  await page.goto("/");
  await expect.poll(async () => (await stubState(page)).watching).toBe(1);
  await stub(page, "abort", { sessionId: "*" });
  await expect(page.getByText("Events were missed")).toBeVisible();
  await page.getByRole("button", { name: "Reconnect" }).click();
  await expect(page.getByText("Live")).toBeVisible();
  await expect(row(page, "RAN-103")).toBeVisible();
});

test("AC1 empty and error states are designed", async ({ page }) => {
  await stub(page, "reset", { sessions: [] });
  await page.goto("/");
  await expect(page.getByRole("heading", { name: "No sessions yet" })).toBeVisible();

  await stub(page, "reset", { fail: true });
  await page.reload();
  await expect(page.getByRole("alert")).toContainText("Can't reach the Rangelight api");
  await expect(page.getByRole("alert")).toContainText("Unavailable: store_unavailable");
  await stub(page, "reset", { sessions: SESSIONS });
  await page.getByRole("button", { name: "Try again" }).click();
  await expect(row(page, "RAN-102")).toBeVisible();
});

test("AC2 the feed is live, and an ABORTED end offers reconnect and resumes", async ({ page }) => {
  await page.goto(`/sessions/${FIRST}`);
  await expect(page.getByRole("heading", { name: "Cart totals round half-even" })).toBeVisible();
  await expect.poll(async () => (await stubState(page)).watching).toBe(1);
  await stub(page, "emit", { sessionId: FIRST, state: DesiredState.WARM });
  const feed = page.getByRole("region", { name: "Feed" });
  await expect(feed).toContainText("State changed to warm");

  await stub(page, "abort", { sessionId: FIRST });
  await expect(page.getByText("Events were missed")).toBeVisible();
  await page.getByRole("button", { name: "Reconnect" }).click();
  await expect(page.getByText("Live")).toBeVisible();
  await expect.poll(async () => (await stubState(page)).watching).toBe(1);
  await stub(page, "emit", { sessionId: FIRST, state: DesiredState.HOT });
  await expect(feed).toContainText("State changed to hot");
  await expect(feed.getByRole("listitem")).toHaveCount(2);
});

test("AC2 an unknown session id says so", async ({ page }) => {
  await page.goto("/sessions/019a5f10-0000-7000-8000-0000000009ff");
  await expect(page.getByRole("heading", { name: "No such session" })).toBeVisible();
});

test("AC3 the terminal speaks terminal.v1alpha1 over the api's WebSocket", async ({ page }) => {
  await page.goto(`/sessions/${FIRST}`);
  const terminal = page.getByRole("region", { name: "Terminal" });
  await expect(terminal.getByRole("status")).toHaveText("Connected");
  await expect.poll(async () => (await stubState(page)).lastResize?.cols ?? 0).toBeGreaterThan(10);
  await page.getByTestId("terminal").click();
  await page.keyboard.type("echo ünïcode");
  await expect(page.locator(".xterm-rows")).toContainText("echo ünïcode");
});

test("AC5 the terminal renders in Geist Mono, not a fallback", async ({ page }) => {
  await page.goto(`/sessions/${FIRST}`);
  await page.getByTestId("terminal").click();
  await page.keyboard.type("font check");
  await expect(page.locator(".xterm-rows")).toContainText("font check");
  // The font Chrome actually drew the row with, whatever the CSS asked for.
  const cdp = await page.context().newCDPSession(page);
  await cdp.send("DOM.enable");
  await cdp.send("CSS.enable");
  const { root } = await cdp.send("DOM.getDocument");
  const { nodeId } = await cdp.send("DOM.querySelector", {
    nodeId: root.nodeId,
    selector: ".xterm-rows > div",
  });
  const { fonts } = await cdp.send("CSS.getPlatformFontsForNode", { nodeId });
  expect(fonts.map((f) => f.familyName)).toEqual(["Geist Mono"]);
});

test("AC5 keyboard reaches the rows with a visible focus ring", async ({ page }) => {
  await page.goto("/");
  await expect(row(page, "RAN-101")).toBeVisible();
  for (let i = 0; i < 6; i++) await page.keyboard.press("Tab");
  const focused = page.locator(":focus-visible");
  await expect(focused).toHaveCount(1);
  expect(await focused.evaluate((el) => getComputedStyle(el).outlineStyle)).toBe("solid");
});

test("AC5 reduced motion stops animations", async ({ page }) => {
  await page.emulateMedia({ reducedMotion: "reduce" });
  await page.goto("/");
  const dot = page.locator(".rl-pulse");
  await expect(dot).toBeVisible();
  expect(await dot.evaluate((el) => getComputedStyle(el).animationIterationCount)).toBe("1");
});

for (const theme of ["light", "dark"] as const) {
  test(`AC5 ${theme}: no serious axe violations, screenshots`, async ({ page }) => {
    await page.emulateMedia({ colorScheme: theme, reducedMotion: "reduce" });
    for (const [name, path] of [
      ["sessions", "/"],
      ["session", `/sessions/${FIRST}`],
    ] as const) {
      await page.goto(path);
      await expect(page.getByRole("heading", { level: 1 })).toBeVisible();
      if (name === "session") {
        await expect(page.getByRole("region", { name: "Terminal" }).getByRole("status")).toHaveText(
          "Connected",
        );
        await expect.poll(async () => (await stubState(page)).watching).toBe(1);
        await stub(page, "emit", {
          sessionId: FIRST,
          state: DesiredState.WARM,
          atMs: Date.UTC(2026, 10, 7, 9, 30),
        });
        await expect(page.getByRole("region", { name: "Feed" }).getByRole("listitem")).toHaveCount(
          1,
        );
        await page.getByTestId("terminal").click();
        await page.keyboard.type("/login");
        await expect(page.locator(".xterm-rows")).toContainText("/login");
      }
      await page.evaluate(() => document.fonts.ready);
      const { violations } = await new AxeBuilder({ page }).analyze();
      expect(
        violations
          .filter((v) => v.impact === "serious" || v.impact === "critical")
          .map((v) => v.id),
      ).toEqual([]);
      await expect(page).toHaveScreenshot(`${name}-${theme}.png`);
    }
  });
}
