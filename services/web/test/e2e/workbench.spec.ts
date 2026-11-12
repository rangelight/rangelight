import AxeBuilder from "@axe-core/playwright";
import { expect, test } from "@playwright/test";
import { DesiredState, Phase } from "@gen/proto/rangelight/runner/v1alpha1/runner_pb.ts";
import { FIRST, row, SESSIONS, stub, stubState } from "./fixtures";

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

test("RAN-56 the PR link shows live, on the row and in the session view", async ({ page }) => {
  const url = "https://github.com/acme/storefront/pull/7";
  await page.goto("/");
  await expect(row(page, "RAN-101").getByRole("link", { name: /pull request/ })).toHaveCount(0);
  await expect.poll(async () => (await stubState(page)).watching).toBe(1);
  await stub(page, "report", { sessionId: FIRST, phase: Phase.RUNNING, pullRequestUrl: url });
  const link = row(page, "RAN-101").getByRole("link", { name: /#7 pull request/ });
  await expect(link).toHaveAttribute("href", url);
  await expect(link).toHaveAttribute("target", "_blank");
  await expect(link).toHaveAttribute("rel", "noopener noreferrer");

  await page.goto(`/sessions/${FIRST}`);
  await expect(
    page.locator("header").getByRole("link", { name: /#7 pull request/ }),
  ).toHaveAttribute("href", url);
});

test("RAN-56 at 360x640 the list and the session view fit, PR link included", async ({ page }) => {
  await page.setViewportSize({ width: 360, height: 640 });
  await page.goto("/");
  await expect.poll(async () => (await stubState(page)).watching).toBe(1);
  await stub(page, "report", {
    sessionId: FIRST,
    phase: Phase.RUNNING,
    pullRequestUrl: "https://github.com/acme/storefront/pull/7",
  });
  const fits = () => page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth);
  await expect(row(page, "RAN-101").getByRole("link", { name: /#7 pull request/ })).toBeVisible();
  expect(await fits()).toBe(true);
  await page.goto(`/sessions/${FIRST}`);
  await expect(page.locator("header").getByRole("link", { name: /#7 pull request/ })).toBeVisible();
  expect(await fits()).toBe(true);
});

const SIGN_IN =
  "Please sign in to Claude: open this lane's terminal, type /login, and answer Yes when Claude asks whether you trust the folder. The lane starts by itself once you're signed in.";

test("RAN-63 an ask shows waiting for you live, on the row and in the session view, until the lane goes on", async ({
  page,
}) => {
  await page.goto("/");
  await expect.poll(async () => (await stubState(page)).watching).toBe(1);
  await stub(page, "agent", { sessionId: FIRST, question: SIGN_IN });
  await expect(row(page, "RAN-101")).toContainText("Waiting for you");
  await stub(page, "agent", { sessionId: FIRST, question: "" });
  await expect(row(page, "RAN-101")).not.toContainText("Waiting for you");

  await page.goto(`/sessions/${FIRST}`);
  await expect.poll(async () => (await stubState(page)).watching).toBe(1);
  await stub(page, "agent", { sessionId: FIRST, question: SIGN_IN });
  await expect(page.locator("header")).toContainText(`Waiting for you: ${SIGN_IN}`);
  await expect(page.getByRole("region", { name: "Feed" })).toContainText(
    `The agent asks: ${SIGN_IN}`,
  );
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

test("RAN-57 a narrow desktop window keeps its header while the terminal has focus", async ({
  page,
}) => {
  await page.setViewportSize({ width: 800, height: 660 });
  await page.goto(`/sessions/${FIRST}`);
  await page.getByTestId("terminal").click();
  await expect(page.locator(".xterm-helper-textarea")).toBeFocused();
  await expect(page.getByRole("heading", { level: 1 })).toBeVisible();
});

// RAN-41 AC6: Claude Code's keys reach the PTY as a local terminal sends them; the browser keeps none.
test("AC3 Escape, Tab, Shift+Tab, Ctrl+C and the arrows reach the terminal's socket", async ({
  page,
}) => {
  await page.goto(`/sessions/${FIRST}`);
  await expect(page.getByRole("region", { name: "Terminal" }).getByRole("status")).toHaveText(
    "Connected",
  );
  await page.getByTestId("terminal").click();
  for (const key of ["Escape", "Tab", "Shift+Tab", "Control+C", "ArrowUp", "ArrowDown", "Enter"]) {
    await page.keyboard.press(key);
  }
  // ESC, TAB, CSI Z, ETX, CSI A, CSI B, CR: in order, nothing lost to focus moves or the page.
  await expect
    .poll(async () => (await stubState(page)).input)
    .toBe("\x1b\t\x1b[Z\x03\x1b[A\x1b[B\r");
  await expect(page.getByTestId("terminal").locator("textarea")).toBeFocused();
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
