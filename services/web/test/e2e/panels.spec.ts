// RAN-84: the session panels and the remaining mock-up views on the preview facade, at desktop
// (1100x660) and at both phone sizes (the phone projects run this file too). The hard rule from
// RAN-83's security review: sample data never shows under a real session's identity.
import { expect, type Locator, type Page, test } from "@playwright/test";
import { axe, FIRST, fitsTheWidth, SESSIONS, stub, stubState, touchTargets } from "./fixtures";

const MARK = "Preview: sample data";
const SAMPLE_ONLY = "Preview: changed in the sample data only. Nothing was sent.";
const QUESTION = "Please sign in to Claude: type /login in this lane's terminal.";
const PANELS = [
  "Diff",
  "Tests",
  "Receipt",
  "Diagnosis",
  "Pod",
  "Preview",
  "Code",
  "Logs",
  "Services",
  "History",
];
const nav = (page: Page) => page.getByRole("navigation", { name: "Primary" });
const press = async (target: Locator, isMobile: boolean) => {
  await (isMobile ? target.tap() : target.click());
};

/** A view from the rail, or from the palette when the rail (or a phone's bar) doesn't keep it. */
async function open(page: Page, label: string, isMobile: boolean): Promise<void> {
  const link = nav(page).getByRole("link", { name: label, exact: true });
  if (await link.isVisible()) {
    await press(link, isMobile);
    return;
  }
  if (isMobile) await nav(page).getByRole("button", { name: "More views" }).tap();
  else await page.keyboard.press("Control+K");
  await press(
    page.getByRole("dialog").getByRole("button", { name: new RegExp(`^${label}`) }),
    isMobile,
  );
}

async function fits(page: Page, isMobile: boolean): Promise<void> {
  await fitsTheWidth(page);
  if (isMobile) await touchTargets(page);
  await axe(page);
}

test.beforeEach(async ({ page }) => {
  await stub(page, "reset", { sessions: SESSIONS });
  await page.emulateMedia({ reducedMotion: "reduce" });
  await page.goto("/");
  await expect.poll(async () => (await stubState(page)).watchingAll).toBe(1);
  await stub(page, "agent", { sessionId: FIRST, question: QUESTION });
});

test("AC1 and the hard rule: on a real session, every panel is a marked example, its actions off", async ({
  page,
  isMobile,
}) => {
  await page.goto(`/sessions/${FIRST}`);
  await expect(page.getByRole("heading", { name: "Cart totals round half-even" })).toBeVisible();
  // The full stage strip and the budget: an example, marked, never this session's. A phone leaves
  // it out on a real session: the terminal needs the rows, and it would only be an example.
  const strip = page.getByRole("group", { name: "Preview: what the stage strip will show" });
  if (isMobile) await expect(strip).toBeHidden();
  else {
    await expect(strip.getByText(MARK)).toBeVisible();
    await expect(strip).toContainText("Example from ACME-41");
  }
  if (isMobile) await page.getByRole("tab", { name: "Feed" }).tap();
  const tabs = page.getByRole("tablist", { name: "Session panels" });
  // The real feed stays real: no mark on it.
  await expect(page.getByRole("region", { name: "Feed" }).getByText(MARK)).toHaveCount(0);
  for (const name of PANELS) {
    await press(tabs.getByRole("tab", { name, exact: true }), isMobile);
    const panel = page.getByRole("tabpanel", { name });
    const example = panel.getByRole("region", { name: "Preview: what this panel will show" });
    await expect(example.getByText(MARK).first()).toBeVisible();
    await expect(example).toContainText("an example from sample lane ACME-41, not this session's");
    // Nothing in the panel outside the marked frame, and no action enabled.
    expect(await panel.evaluate((el) => [...el.children].length)).toBe(1);
    await expect(panel.locator("button:enabled")).toHaveCount(0);
  }
  await fits(page, isMobile);
});

test("AC1 on a sample lane the panels act on the sample world, and say so", async ({
  page,
  isMobile,
}) => {
  await page.goto("/lanes/ACME-41");
  await expect(
    page.getByRole("heading", { name: "Refund credits when a checkout times out" }),
  ).toBeVisible();
  await expect(page.locator("header").getByText(MARK)).toBeVisible();
  const tabs = page.getByRole("tablist", { name: "Session panels" });
  await press(tabs.getByRole("tab", { name: "Tests", exact: true }), isMobile);
  const tests = page.getByRole("tabpanel", { name: "Tests" });
  await expect(tests.getByText(MARK)).toBeVisible();
  await press(tests.getByRole("button", { name: "Re-run failing" }), isMobile);
  await expect(page.getByRole("status").filter({ hasText: SAMPLE_ONLY })).toBeVisible();
  await expect(tests).toContainText("6 of 6 pass");
  // The receipt sees the same world: the rounding criterion is proven now.
  await press(tabs.getByRole("tab", { name: "Receipt", exact: true }), isMobile);
  await expect(page.getByRole("tabpanel", { name: "Receipt" })).toContainText("4/5 proven");
  await press(tabs.getByRole("tab", { name: "History", exact: true }), isMobile);
  await press(page.getByRole("button", { name: "Fork from the latest checkpoint" }), isMobile);
  await press(tabs.getByRole("tab", { name: "Events", exact: true }), isMobile);
  await expect(page.getByRole("list", { name: "Feed" })).toContainText(
    "Re-ran the failing tests: all pass",
  );
  await fits(page, isMobile);
});

test("AC2 Home, Repositories, Settings and Claude access: real facts real, the rest marked", async ({
  page,
  isMobile,
}) => {
  await open(page, "Home", isMobile);
  await expect(page.getByRole("link", { name: /1\s*need you now/ })).toBeVisible();
  await expect(page.getByRole("link", { name: /need you now/ }).getByText(MARK)).toHaveCount(0);
  await fits(page, isMobile);

  await open(page, "Repositories", isMobile);
  const yours = page.getByRole("region", { name: "Your repos" });
  await expect(yours).toContainText("acme/storefront");
  await expect(yours.getByText(MARK)).toHaveCount(0);
  await expect(page.getByRole("region", { name: "Sample repos" }).getByText(MARK)).toBeVisible();
  await fits(page, isMobile);

  await open(page, "Settings", isMobile);
  await expect(page.getByLabel("Account")).toContainText("ariel");
  await expect(page.getByLabel("Account").getByText(MARK)).toHaveCount(0);
  await expect(page.getByRole("region", { name: "Team settings" }).getByText(MARK)).toBeVisible();
  await press(page.getByRole("button", { name: "Save the team settings" }), isMobile);
  await expect(page.getByRole("status").filter({ hasText: SAMPLE_ONLY })).toBeVisible();
  await fits(page, isMobile);

  await open(page, "Claude access", isMobile);
  const lanes = page.getByRole("region", { name: "Your lanes" });
  await expect(lanes).toContainText(`Waiting for you: ${QUESTION}`);
  await expect(lanes).toContainText("Its agent has no open question.");
  await expect(lanes.getByText(MARK)).toHaveCount(0);
  await expect(page.getByRole("region", { name: "Team access" }).getByText(MARK)).toBeVisible();
  await fits(page, isMobile);
});

test("AC2 Fork compare and the Retro card are marked sample views; keeping a run says so", async ({
  page,
  isMobile,
}) => {
  await open(page, "Fork compare", isMobile);
  await expect(page.locator("header").getByText(MARK)).toBeVisible();
  await press(page.getByRole("button", { name: "Keep A" }), isMobile);
  await expect(page.getByRole("status").filter({ hasText: SAMPLE_ONLY })).toBeVisible();
  await expect(page.getByRole("button", { name: "Kept" })).toBeDisabled();
  await fits(page, isMobile);

  await open(page, "Retro", isMobile);
  await expect(page.locator("header").getByText(MARK)).toBeVisible();
  await expect(page.getByRole("list", { name: "Timeline" })).toContainText("Review");
  await fits(page, isMobile);
});
