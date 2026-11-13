// RAN-85: the guided story, started from the palette, steps through every chapter on the sample
// world at desktop and at both phone sizes; leaving it puts the sample world back, and the real
// sessions and the terminal are never touched.
import { expect, type Page, test } from "@playwright/test";
import { axe, fitsTheWidth, row, SESSIONS, stub, stubState } from "./fixtures";

const MARK = "Preview: sample data";
const bar = (page: Page) => page.getByRole("region", { name: "Guided story" });

/** What each chapter shows once its steps ran: the view, and something its action changed. */
const SHOWS: [string, RegExp, (page: Page) => Promise<void>][] = [
  [
    "Morning",
    /\/architect$/,
    async (p) => {
      await expect(p.getByText(/watching \d+ lanes, and 3 sample/)).toBeVisible();
    },
  ],
  [
    "Start from Up next",
    /\/architect$/,
    async (p) => {
      await expect(p.getByText(/watching \d+ lanes, and 4 sample/)).toBeVisible();
    },
  ],
  [
    "Cross-repo build",
    /\/decisions$/,
    async (p) => {
      const decided = p.getByRole("region", { name: "Decided today" });
      await expect(decided).toContainText("Grant read access to the error tracker");
    },
  ],
  [
    "The loop closes itself",
    /\/decisions$/,
    async (p) => {
      const needs = p.getByRole("region", { name: "Needs you" });
      await expect(needs).toContainText("Third QA bounce");
    },
  ],
  [
    "Bounded autonomy",
    /\/decisions$/,
    async (p) => {
      const decided = p.getByRole("region", { name: "Decided today" });
      await expect(decided).toContainText("Third QA bounce");
    },
  ],
  [
    "Budget wall",
    /\/decisions$/,
    async (p) => {
      const decided = p.getByRole("region", { name: "Decided today" });
      await expect(decided).toContainText("Raise the lane's budget");
    },
  ],
  [
    "Fork",
    /\/architect$/,
    async (p) => {
      await expect(p.getByText(/watching \d+ lanes, and 5 sample/)).toBeVisible();
    },
  ],
  [
    "Proof",
    /\/reviews$/,
    async (p) => {
      // Read the pull request's story to its end, where its approval shows.
      await p.getByRole("button", { name: "4. How it was verified" }).click();
      await expect(p.getByRole("button", { name: "Approved" })).toBeVisible();
    },
  ],
];

test.beforeEach(async ({ page }) => {
  await stub(page, "reset", { sessions: SESSIONS });
  await page.emulateMedia({ reducedMotion: "reduce" });
});

test("RAN-85 the guided story plays every chapter on the sample world, and leaving resets it", async ({
  page,
  isMobile,
}) => {
  await page.goto("/");
  await expect(row(page, "RAN-101")).toBeVisible();
  const palette = page.getByRole("dialog", { name: "Command palette" });
  if (isMobile) await page.getByRole("button", { name: "Search" }).tap();
  else await page.keyboard.press("Control+K");
  await palette.getByRole("button", { name: /Play the guided story/ }).click();
  await expect(bar(page)).toContainText(MARK);

  for (const [n, [title, url, shows]] of SHOWS.entries()) {
    await expect(bar(page).getByRole("heading")).toHaveText(title);
    await expect(bar(page)).toContainText(`${String(n + 1)}/${String(SHOWS.length)}`);
    await expect(page).toHaveURL(url);
    await shows(page);
    // The chapter has run all its steps: it offers a replay.
    await expect(bar(page).getByRole("button", { name: "Replay the chapter" })).toBeVisible();
    await fitsTheWidth(page);
    if (n < SHOWS.length - 1)
      await bar(page).getByRole("button", { name: "Next story chapter" }).click();
  }
  await axe(page);

  // Back replays the previous chapter from its start.
  await bar(page).getByRole("button", { name: "Previous story chapter" }).click();
  await expect(bar(page).getByRole("heading")).toHaveText("Fork");
  await expect(page.getByText(/watching \d+ lanes, and 5 sample/)).toBeVisible();

  // Pause holds a chapter mid-way: Proof's approval doesn't come until Play.
  await bar(page).getByRole("button", { name: "Next story chapter" }).click();
  await bar(page).getByRole("button", { name: "Pause" }).click();
  await page.getByRole("button", { name: "4. How it was verified" }).click();
  await page.waitForTimeout(2500);
  await expect(page.getByRole("button", { name: "Approve", exact: true })).toBeVisible();
  await bar(page).getByRole("button", { name: "Play" }).click();
  await expect(page.getByRole("button", { name: "Approved" })).toBeVisible();

  // Leaving: the bar goes, the sample world is as it starts and live again.
  await bar(page).getByRole("button", { name: "Leave the story" }).click();
  await expect(bar(page)).toBeHidden();
  await expect(page.getByRole("button", { name: "Approve", exact: true })).toBeVisible();

  // The real sessions were never touched: the list is the api's, and no terminal input was sent.
  await page.goto("/");
  await expect(row(page, "RAN-101")).toBeVisible();
  expect((await stubState(page)).input).toBe("");
});
