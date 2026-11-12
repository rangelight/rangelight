// RAN-75: the Fleet wall and Up next, at desktop (1100x660) and at both phone sizes (the phone
// projects run this file too).
import { expect, type Locator, type Page, test } from "@playwright/test";
import { DesiredState, Phase } from "@gen/proto/rangelight/runner/v1alpha1/runner_pb.ts";
import { axe, FIRST, fitsTheWidth, SESSIONS, stub, stubState, touchTargets } from "./fixtures";

const SECOND = "019a5f10-0000-7000-8000-000000000102";
const NEW = "019a5f10-0000-7000-8000-000000000104";
const nav = (page: Page) => page.getByRole("navigation", { name: "Primary" });
const press = async (target: Locator, isMobile: boolean) => {
  await (isMobile ? target.tap() : target.click());
};

/** RAN-101 runs, RAN-102 is queued, RAN-103 is torn down, and RAN-104 has just been created. */
async function fleet(page: Page): Promise<void> {
  await expect.poll(async () => (await stubState(page)).watchingAll).toBe(1);
  await stub(page, "report", { sessionId: FIRST, phase: Phase.RUNNING });
  await stub(page, "report", { sessionId: SECOND, phase: Phase.QUEUED });
  await stub(page, "create", {
    id: NEW,
    ticketKey: "RAN-104",
    title: "Invoices list paginates past 1000 rows",
    repo: "",
    state: DesiredState.HOT,
    minutesAgo: 0,
  });
}

test.beforeEach(async ({ page }) => {
  await stub(page, "reset", { sessions: SESSIONS });
  await page.emulateMedia({ reducedMotion: "reduce" });
});

test("AC1 the Fleet wall: a tile per live lane, its state, phase, activity and who waits", async ({
  page,
  isMobile,
}) => {
  await page.goto("/");
  if (isMobile) {
    // A phone's bottom bar keeps three views; More opens the palette with all of them (RAN-83).
    await nav(page).getByRole("button", { name: "More views" }).tap();
    await page
      .getByRole("dialog")
      .getByRole("button", { name: /Fleet wall/ })
      .tap();
  } else await nav(page).getByRole("link", { name: "Fleet wall" }).click();
  await expect(page).toHaveURL(/\/wall$/);
  if (!isMobile)
    await expect(nav(page).getByRole("link", { name: "Fleet wall" })).toHaveAttribute(
      "aria-current",
      "page",
    );
  await expect(page.getByRole("heading", { name: "No lane is running" })).toBeVisible();
  await fleet(page);
  const tiles = page.getByRole("main").getByRole("listitem");
  await expect(tiles).toHaveCount(1); // Only RAN-101's lane has started.
  const tile = tiles.first();
  await expect(tile).toContainText("RAN-101");
  await expect(tile).toContainText("Hot");
  await expect(tile).toContainText("Running");
  await expect(tile).toContainText(/Last activity \d+s ago/);
  await stub(page, "agent", { sessionId: FIRST, question: "Which branch should I base this on?" });
  await expect(tile).toContainText("Waiting for you");
  await expect(tile).toContainText("Which branch should I base this on?");
  await fitsTheWidth(page);
  if (isMobile) await touchTargets(page); // 44 px is the touch and narrow-screen rule.
  await axe(page);
  await press(tile.getByRole("link"), isMobile);
  await expect(page).toHaveURL(new RegExp(`${FIRST}$`));
});

test("AC2 Up next: the sessions with no lane yet, oldest first, linked to Linear and their page", async ({
  page,
  isMobile,
}) => {
  await page.goto("/");
  await fleet(page);
  // From the palette, as from the rail.
  if (isMobile) await page.getByRole("button", { name: "Search" }).tap();
  else await page.keyboard.press("Control+K");
  await page.getByRole("searchbox").fill("up next");
  if (isMobile)
    await page
      .getByRole("dialog")
      .getByRole("button", { name: /Up next/ })
      .tap();
  else await page.keyboard.press("Enter");
  await expect(page).toHaveURL(/\/up-next$/);
  const rows = page.getByRole("list", { name: "Up next, oldest first" }).getByRole("listitem");
  await expect(rows).toHaveCount(2); // RAN-101 runs; RAN-103 is torn down.
  await expect(rows.nth(0)).toContainText("RAN-102");
  await expect(rows.nth(0)).toContainText("Queued");
  await expect(rows.nth(1)).toContainText("RAN-104");
  const ticket = rows.nth(0).getByRole("link", { name: /RAN-102/ });
  await expect(ticket).toHaveAttribute("href", "https://linear.app/example/issue/RAN-102");
  await expect(ticket).toHaveAttribute("target", "_blank");
  await fitsTheWidth(page);
  if (isMobile) await touchTargets(page); // 44 px is the touch and narrow-screen rule.
  await axe(page);
  await press(
    rows.nth(1).getByRole("link", { name: "Invoices list paginates past 1000 rows" }),
    isMobile,
  );
  await expect(page).toHaveURL(new RegExp(`${NEW}$`));
});
