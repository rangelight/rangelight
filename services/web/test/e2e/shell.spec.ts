// RAN-74: the mock-up's app shell, at desktop (1100x660) and at both phone sizes (the phone
// projects run this file too). A phone has no fleet column (a sheet instead) and a bottom bar.
import { expect, type Page, test } from "@playwright/test";
import { Phase } from "@gen/proto/rangelight/runner/v1alpha1/runner_pb.ts";
import { axe, FIRST, fitsTheWidth, row, SESSIONS, stub, stubState, touchTargets } from "./fixtures";

const SESSION = `/sessions/${FIRST}`;
const nav = (page: Page) => page.getByRole("navigation", { name: "Primary" });
const view = (page: Page) => page.getByLabel("Current view");
/** A tap on a phone, a click on the desktop. */
const press = async (page: Page, target: ReturnType<Page["getByRole"]>, isMobile: boolean) => {
  await (isMobile ? target.tap() : target.click());
};

test.beforeEach(async ({ page }) => {
  await stub(page, "reset", { sessions: SESSIONS });
  await page.emulateMedia({ reducedMotion: "reduce" });
});

test("AC1 the rail has Sessions and the session last opened, and no dead links", async ({
  page,
  isMobile,
}) => {
  await page.goto("/");
  // Sessions, the Fleet wall and Up next (RAN-75); no session opened yet.
  await expect(nav(page).getByRole("link")).toHaveCount(3);
  await expect(nav(page).getByRole("link", { name: "Sessions" })).toHaveAttribute(
    "aria-current",
    "page",
  );
  await press(
    page,
    row(page, "RAN-101").getByRole("link", { name: "Cart totals round half-even" }),
    isMobile,
  );
  const current = nav(page).getByRole("link", { name: "Current session" });
  await expect(current).toHaveAttribute("aria-current", "page");
  await press(page, nav(page).getByRole("link", { name: "Sessions" }), isMobile);
  await expect(page).toHaveURL(/\/$/);
  // Back to it from the rail.
  await press(page, current, isMobile);
  await expect(page).toHaveURL(new RegExp(`${SESSION}$`));
  await expect(nav(page).getByRole("link")).toHaveCount(4);
});

test("AC2 the top bar: the view, the live status, who is signed in, and sign-out", async ({
  page,
  isMobile,
}) => {
  await page.goto(SESSION);
  await expect(view(page)).toContainText("Session");
  await expect(view(page)).toContainText("RAN-101");
  const status = page
    .getByRole("status")
    .filter({ hasText: /Connected|Reconnecting/ })
    .first();
  await expect(status).toContainText("Connected");
  await expect.poll(async () => (await stubState(page)).watchingAll).toBe(1);
  await stub(page, "abort", { sessionId: "*" });
  await expect(status).toContainText("Reconnecting…");
  await expect(page.getByText("Signed in as ariel")).toBeAttached();
  await press(page, page.getByRole("button", { name: "Sign out" }), isMobile);
  await expect(page.getByText("You're signed out")).toBeVisible();
  expect((await stubState(page)).signedOut).toBe(true);
});

test("AC2 a sign-out the api refuses keeps the page, and says you are still signed in", async ({
  page,
  isMobile,
}) => {
  await stub(page, "reset", { sessions: SESSIONS, logoutFails: true });
  await page.goto("/");
  await expect(row(page, "RAN-101")).toBeVisible();
  await press(page, page.getByRole("button", { name: "Sign out" }), isMobile);
  await expect(page.getByRole("alert")).toContainText("Sign out failed: you're still signed in.");
  await expect(page).toHaveURL(/\/$/);
  expect((await stubState(page)).signedOut).toBe(false);
  await fitsTheWidth(page);
  await axe(page);
  // The api recovers: the same button signs out.
  await stub(page, "reset", { sessions: SESSIONS });
  await press(page, page.getByRole("button", { name: "Sign out" }), isMobile);
  await expect(page.getByText("You're signed out")).toBeVisible();
});

test("RAN-82 sign out stays signed out until Sign in, which asks the provider to confirm, once", async ({
  page,
  isMobile,
}) => {
  await page.goto(SESSION);
  await expect(page.getByText("Signed in as ariel")).toBeAttached();
  await press(page, page.getByRole("button", { name: "Sign out" }), isMobile);
  // The signed-out page, and it stays: no request to sign in, even on a reload in this tab.
  await expect(page).toHaveURL(/\/$/);
  await expect(page.getByText("You're signed out")).toBeVisible();
  await page.waitForTimeout(1500);
  await page.reload();
  await expect(page.getByText("You're signed out")).toBeVisible();
  await page.waitForTimeout(1500);
  expect((await stubState(page)).signIns).toEqual([]);
  await fitsTheWidth(page);
  await axe(page);
  // Sign in: one redirect, asking for the provider's confirmation.
  await press(page, page.getByRole("button", { name: "Sign in" }), isMobile);
  await expect(page).toHaveURL(/\/auth\/login\?prompt=consent$/);
  await page.waitForTimeout(500);
  expect((await stubState(page)).signIns).toEqual(["/auth/login?prompt=consent"]);
  // Another tab, still signed out, goes to sign in once, as before (RAN-71).
  const other = await page.context().newPage();
  await other.goto("/");
  await expect(other).toHaveURL(/\/auth\/login$/);
});

test("AC3 the fleet lists live sessions with their state and who waits; it collapses, remembered", async ({
  page,
  isMobile,
}) => {
  await page.goto("/");
  await expect.poll(async () => (await stubState(page)).watchingAll).toBe(1);
  await stub(page, "agent", { sessionId: FIRST, question: "Which branch?" });
  if (isMobile) {
    await page.getByRole("button", { name: "Fleet" }).tap();
  }
  const fleet = isMobile
    ? page.getByRole("dialog", { name: "Fleet" })
    : page.getByRole("complementary", { name: "Fleet" });
  const listed = fleet.getByRole("link");
  await expect(listed).toHaveCount(2); // RAN-103 is cold: torn down, not live.
  await expect(listed.filter({ hasText: "RAN-101" })).toContainText("Hot");
  await expect(listed.filter({ hasText: "RAN-101" })).toContainText("Waiting for you");
  await expect(listed.filter({ hasText: "RAN-102" })).toContainText("Warm");
  if (isMobile) {
    // The sheet: a session picked from it opens, and the sheet closes.
    await fitsTheWidth(page);
    await listed.filter({ hasText: "RAN-102" }).tap();
    await expect(page).toHaveURL(/000000000102$/);
    await expect(fleet).toBeHidden();
    return;
  }
  await page.getByRole("button", { name: "Collapse fleet" }).click();
  await expect(fleet).toBeHidden();
  await page.reload();
  await expect(row(page, "RAN-101")).toBeVisible();
  await expect(fleet).toBeHidden();
  await page.getByRole("button", { name: "Expand fleet" }).click();
  await expect(fleet).toBeVisible();
  await page.keyboard.press("Control+B");
  await expect(fleet).toBeHidden();
});

test("AC4 the stage strip shows the lane's phase; the terminal stays", async ({ page }) => {
  await page.goto(SESSION);
  const strip = page.getByRole("list", { name: "Lane phase" });
  await expect(strip).toContainText("Not reported");
  await expect.poll(async () => (await stubState(page)).watching).toBe(1);
  await stub(page, "report", { sessionId: FIRST, phase: Phase.RUNNING });
  await expect(strip.locator('[aria-current="step"]')).toHaveText("Running");
  await expect(page.getByTestId("terminal")).toBeVisible();
});

test("AC5 the command palette jumps to a session or a view; ? shows the keys", async ({
  page,
  isMobile,
}) => {
  await page.goto("/");
  await expect(row(page, "RAN-102")).toBeVisible();
  const palette = page.getByRole("dialog", { name: "Command palette" });
  if (isMobile) await page.getByRole("button", { name: "Search" }).tap();
  else await page.keyboard.press("Control+K");
  await expect(palette).toBeVisible();
  await page.getByRole("searchbox", { name: "Jump to a session or a view" }).fill("webhook");
  if (isMobile) await palette.getByRole("button", { name: /RAN-102/ }).tap();
  else await page.keyboard.press("Enter");
  await expect(page).toHaveURL(/000000000102$/);
  await expect(palette).toBeHidden();
  if (isMobile) return; // No keyboard for `?` on a phone: the shortcuts are a keyboard's.
  await page.keyboard.press("Control+K");
  await page.getByRole("searchbox").fill("sessions");
  await page.keyboard.press("Enter");
  await expect(page).toHaveURL(/\/$/);
  await page.keyboard.press("?");
  const keys = page.getByRole("dialog", { name: "Keyboard shortcuts" });
  await expect(keys).toContainText("Command palette");
  await page.keyboard.press("Escape");
  await expect(keys).toBeHidden();
});

test("AC6 the shell on a phone: a bottom bar, nothing sideways, 44 px targets, the terminal fills", async ({
  page,
  isMobile,
}) => {
  test.skip(!isMobile, "phone sizes only");
  for (const path of ["/", SESSION]) {
    await page.goto(path);
    await expect(page.getByRole("heading", { level: 1 })).toBeVisible();
    const bar = await nav(page).boundingBox();
    expect((bar?.y ?? 0) + (bar?.height ?? 0)).toBeCloseTo(page.viewportSize()?.height ?? 0, 0);
    await expect(() => fitsTheWidth(page)).toPass({ timeout: 5_000 });
    await touchTargets(page);
    await axe(page);
  }
  // The terminal takes the rest of the view: it ends where the key bar starts, above the bottom bar.
  const terminal = await page.getByTestId("terminal").boundingBox();
  const keys = await page.getByRole("group", { name: "Terminal keys" }).boundingBox();
  // (Within the key bar's 1 px top border.)
  expect(
    Math.abs((terminal?.y ?? 0) + (terminal?.height ?? 0) - (keys?.y ?? 0)),
  ).toBeLessThanOrEqual(2);
  expect(terminal?.height ?? 0).toBeGreaterThan(150);
  // Typing (the soft keyboard up): the shell's bars step aside for the terminal.
  await page.getByTestId("terminal").tap();
  await expect(nav(page)).toBeHidden();
});
