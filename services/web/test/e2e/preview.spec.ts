// RAN-83: the mock-up's Architect home, Coordinator, Decisions inbox and Reviews on the preview
// facade, at desktop (1100x660) and at both phone sizes (the phone projects run this file too).
// Sample data carries its mark, real data stays real, and actions change the sample world only.
import { expect, type Locator, type Page, test } from "@playwright/test";
import { Phase } from "@gen/proto/rangelight/runner/v1alpha1/runner_pb.ts";
import { axe, FIRST, fitsTheWidth, SESSIONS, stub, stubState, touchTargets } from "./fixtures";

const MARK = "Preview: sample data";
const SAMPLE_ONLY = "Preview: changed in the sample data only. Nothing was sent.";
const QUESTION = "Which branch should I base this on?";
const nav = (page: Page) => page.getByRole("navigation", { name: "Primary" });
const press = async (target: Locator, isMobile: boolean) => {
  await (isMobile ? target.tap() : target.click());
};

/** A view from the rail, or on a phone from More (the palette) when its bar doesn't keep it. */
async function open(page: Page, label: string, isMobile: boolean): Promise<void> {
  const link = nav(page).getByRole("link", { name: label, exact: true });
  if (!isMobile || (await link.isVisible())) await press(link, isMobile);
  else {
    await nav(page).getByRole("button", { name: "More views" }).tap();
    await page
      .getByRole("dialog")
      .getByRole("button", { name: new RegExp(`^${label}`) })
      .tap();
  }
}

/** On a phone: nothing sideways, 44 px targets, no serious axe finding. */
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
  // A real lane asks: real data, which the preview must show as real.
  await stub(page, "agent", { sessionId: FIRST, question: QUESTION });
});

test("AC4 the Architect: the real question first and unmarked, the sample stream marked", async ({
  page,
  isMobile,
}) => {
  await open(page, "Architect", isMobile);
  await expect(page).toHaveURL(/\/architect$/);
  const real = page.getByRole("region", { name: QUESTION });
  await expect(real.getByRole("link", { name: "Answer in its terminal →" })).toHaveAttribute(
    "href",
    `/sessions/${FIRST}`,
  );
  await expect(real.getByText(MARK)).toHaveCount(0);
  const sample = page.getByRole("region", {
    name: "Third QA bounce on refund › timeout after charge",
  });
  await expect(sample.getByText(MARK)).toBeVisible();
  await expect(
    page.getByRole("article", { name: /Sequenced ACME-42/ }).getByText(MARK),
  ).toBeVisible();
  await fits(page, isMobile);
  // Deciding changes the sample world only, and says so.
  await press(sample.getByRole("button", { name: /Round half to even/ }), isMobile);
  await expect(page.getByRole("status").filter({ hasText: SAMPLE_ONLY })).toBeVisible();
  await expect(sample).toBeHidden();
  // The architect strip on a lane: the real question, before any sample one.
  await page.goto(`/sessions/${FIRST}`);
  await expect(page.getByText(`RAN-101 needs you: ${QUESTION}`)).toBeVisible();
});

test("AC4 the Decisions inbox: a decision taken here is taken in every view", async ({
  page,
  isMobile,
}) => {
  await open(page, "Decisions", isMobile);
  const needs = page.getByRole("region", { name: "Needs you" });
  await expect(needs.getByRole("region", { name: QUESTION })).toBeVisible();
  const bounce = needs.getByRole("region", { name: /Third QA bounce/ });
  await expect(bounce.getByText(MARK)).toBeVisible();
  await expect(page.getByRole("region", { name: "Waiting on others" })).toContainText("Priya Shah");
  await fits(page, isMobile);
  await press(bounce.getByRole("button", { name: /Stop the lane and take it over/ }), isMobile);
  await expect(page.getByRole("region", { name: "Decided today" })).toContainText(
    "you chose 3. Stop the lane and take it over",
  );
  // The same sample world everywhere: the Architect no longer asks, and the lane's feed says it.
  await open(page, "Architect", isMobile);
  await expect(page.getByRole("region", { name: /Third QA bounce/ })).toHaveCount(0);
  await open(page, "Coordinator", isMobile);
  await page.getByRole("combobox", { name: "Lane" }).selectOption("ACME-41");
  await expect(page.getByRole("list", { name: "Feed" })).toContainText(
    "You chose: Stop the lane and take it over",
  );
});

test("AC4 Reviews: a real PR links to GitHub; a sample one reads as a story and approves", async ({
  page,
  isMobile,
}) => {
  await stub(page, "report", {
    sessionId: FIRST,
    phase: Phase.RUNNING,
    pullRequestUrl: "https://github.com/acme/storefront/pull/7",
  });
  await open(page, "Reviews", isMobile);
  const list = page.getByRole("list", { name: "Pull requests" });
  const real = list.getByRole("listitem").filter({ hasText: "#7" });
  await expect(real.getByText(MARK)).toHaveCount(0);
  await press(real.getByRole("button"), isMobile);
  // L2: only the api's fields; no made-up author on a real PR.
  await expect(page.getByRole("region", { name: "Cart totals round half-even" })).not.toContainText(
    "lane ",
  );
  await expect(page.getByRole("link", { name: /Open #7 on GitHub/ })).toHaveAttribute(
    "href",
    "https://github.com/acme/storefront/pull/7",
  );
  const sample = list.getByRole("listitem").filter({ hasText: "ACME-42" });
  await expect(sample.getByText(MARK)).toBeVisible();
  await press(sample.getByRole("button"), isMobile);
  for (let i = 0; i < 3; i++)
    await press(page.getByRole("button", { name: "Next chapter" }), isMobile);
  await fits(page, isMobile);
  await press(page.getByRole("button", { name: "Approve" }), isMobile);
  await expect(page.getByRole("status").filter({ hasText: SAMPLE_ONLY })).toBeVisible();
  await expect(page.getByRole("button", { name: "Approved" })).toBeDisabled();
});

test("AC4 the Coordinator: a real lane shows only real data; the sample lanes talk and act", async ({
  page,
  isMobile,
}) => {
  await open(page, "Coordinator", isMobile);
  const lane = page.getByRole("combobox", { name: "Lane" });
  await expect(lane).toHaveValue(FIRST);
  // RAN-83 security M1: on a real session no sample text, no mark, and a disabled composer that
  // points at its terminal.
  const real = page.getByRole("region", { name: "RAN-101 coordinator" });
  await expect(real.getByRole("region", { name: QUESTION })).toBeVisible();
  await expect(real.getByText(MARK)).toHaveCount(0);
  for (const sample of ["Contract locked", "Workspace ready", "fold that into"])
    await expect(real).not.toContainText(sample);
  await expect(page.getByRole("textbox", { name: "Message the coordinator" })).toBeDisabled();
  await expect(page.getByRole("button", { name: "Send" })).toBeDisabled();
  await expect(
    real.getByRole("link", { name: "Talk to this lane in its terminal →" }),
  ).toHaveAttribute("href", `/sessions/${FIRST}`);
  await fits(page, isMobile);
  // A sample lane: its feed is marked, item by item; a word gets the sample agent's answer.
  await lane.selectOption("ACME-42");
  const feed = page.getByRole("list", { name: "Feed" });
  await expect(feed.getByRole("listitem").first().getByText(MARK)).toBeVisible();
  await page.getByRole("textbox", { name: "Message the coordinator" }).fill("Use the main branch.");
  await press(page.getByRole("button", { name: "Send" }), isMobile);
  await expect(feed).toContainText("Use the main branch.");
  await expect(feed).toContainText("fold that into the current loop");
  await expect(page.getByRole("status").filter({ hasText: SAMPLE_ONLY })).toBeVisible();
  await press(page.getByRole("button", { name: "Pause" }), isMobile);
  await expect(feed).toContainText("Paused by you");
  await press(page.getByRole("button", { name: "Resume" }), isMobile);
  await press(page.getByRole("button", { name: "Fork" }), isMobile);
  await expect(lane.getByRole("option", { name: /ACME-42·b/ })).toBeAttached();
});

test("Amendment: the sample world moves on by itself; lanes go through their stages", async ({
  page,
  isMobile,
}) => {
  test.slow();
  await open(page, "Coordinator", isMobile);
  await page.getByRole("combobox", { name: "Lane" }).selectOption("ACME-44");
  // Verify is its last stage: a few ticks later it is done.
  await expect(page.getByRole("list", { name: "Feed" })).toContainText("Verified in production", {
    timeout: 30_000,
  });
});
