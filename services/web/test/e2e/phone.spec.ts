// The Workbench on a phone: portrait, touch, at the sizes of playwright.config.ts's phone projects.
import AxeBuilder from "@axe-core/playwright";
import { expect, type Page, test } from "@playwright/test";
import { DesiredState, Phase } from "@gen/proto/rangelight/runner/v1alpha1/runner_pb.ts";
import { FIRST, row, SESSIONS, stub, stubState } from "./fixtures";

const SESSION = `/sessions/${FIRST}`;
const textarea = (page: Page) => page.locator(".xterm-helper-textarea");

/** No horizontal page scroll, and nothing reaches past the screen's edge but the key bar's keys,
 * which scroll in their own strip. */
async function fitsTheWidth(page: Page): Promise<void> {
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
async function touchTargets(page: Page): Promise<void> {
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

async function axe(page: Page): Promise<void> {
  const { violations } = await new AxeBuilder({ page }).analyze();
  expect(
    violations.filter((v) => v.impact === "serious" || v.impact === "critical").map((v) => v.id),
  ).toEqual([]);
}

async function connected(page: Page): Promise<void> {
  await expect(page.getByRole("region", { name: "Terminal" }).getByRole("status")).toHaveText(
    "Connected",
  );
  await expect.poll(async () => (await stubState(page)).lastResize?.cols ?? 0).toBeGreaterThan(10);
}

test.beforeEach(async ({ page }) => {
  await stub(page, "reset", { sessions: SESSIONS });
  // axe reads colours mid fade-in otherwise.
  await page.emulateMedia({ reducedMotion: "reduce" });
});

test("AC1 the sessions list fits the screen, every target a thumb's size", async ({ page }) => {
  await page.goto("/");
  await expect(row(page, "RAN-103")).toContainText("no repo yet");
  await expect(row(page, "RAN-101")).toContainText("2h 05m");
  await fitsTheWidth(page);
  await touchTargets(page);
  await axe(page);
  // No rail on a phone: its two items are in the header.
  const nav = page.locator("header").getByRole("navigation", { name: "Primary" });
  await expect(nav.getByRole("link", { name: "Sessions" })).toBeVisible();
  await nav.getByRole("button", { name: "Dark theme" }).tap();
  await expect(page.locator("html")).toHaveClass(/dark/);
});

test("AC1 the session view: terminal and feed as tabs, each fits the screen", async ({ page }) => {
  await page.goto(SESSION);
  await connected(page);
  const feed = page.getByRole("region", { name: "Feed" });
  await expect(page.getByRole("tab", { name: "Terminal" })).toHaveAttribute(
    "aria-selected",
    "true",
  );
  await expect(feed).toBeHidden();
  await fitsTheWidth(page);
  await touchTargets(page);
  await axe(page);

  await page.getByRole("tab", { name: "Feed" }).tap();
  await expect(feed).toBeVisible();
  await expect(page.getByTestId("terminal")).toBeHidden();
  await expect.poll(async () => (await stubState(page)).watching).toBe(1);
  await stub(page, "emit", { sessionId: FIRST, state: DesiredState.WARM });
  await expect(feed).toContainText("State changed to warm");
  await fitsTheWidth(page);
  await touchTargets(page);
  await axe(page);

  // Hidden, the terminal kept its socket.
  await page.getByRole("tab", { name: "Terminal" }).tap();
  await expect(page.getByRole("region", { name: "Terminal" }).getByRole("status")).toHaveText(
    "Connected",
  );
});

test("AC2 a tap opens the keyboard, and text reaches the PTY as typed", async ({ page }) => {
  await page.goto(SESSION);
  await connected(page);
  await page.getByTestId("terminal").tap();
  await expect(textarea(page)).toBeFocused();
  // The keyboard is up: the header makes room for the terminal, and comes back after.
  await expect(page.getByRole("heading", { level: 1 })).toBeHidden();
  await page.getByRole("tab", { name: "Feed" }).tap();
  await expect(page.getByRole("heading", { level: 1 })).toBeVisible();
  await page.getByRole("tab", { name: "Terminal" }).tap();
  await page.getByTestId("terminal").tap();
  await expect(textarea(page)).toHaveAttribute("autocorrect", "off");
  await expect(textarea(page)).toHaveAttribute("autocapitalize", "off");
  await expect(textarea(page)).toHaveAttribute("spellcheck", "false");
  await page.keyboard.type("ls");
  // What an autocorrect or a finished IME composition hands the page: inserted text, not keys.
  await page.keyboard.insertText(" café 日本");
  await expect.poll(async () => (await stubState(page)).input).toBe("ls café 日本");
});

test("AC2 the key bar sends a desktop xterm's bytes and keeps the keyboard up", async ({
  page,
}) => {
  await page.goto(SESSION);
  await connected(page);
  await page.getByTestId("terminal").tap();
  const keys = page.getByRole("group", { name: "Terminal keys" });
  const fade = (side: string) => page.locator(`[data-fade=${side}]`);
  // More keys than width: the fade marks the side they hide on.
  await expect(fade("left")).toHaveCSS("opacity", "0");
  await expect(fade("right")).toHaveCSS("opacity", "1");
  for (const name of [
    "Esc",
    "Shift+Tab",
    "Up arrow",
    "Down arrow",
    "Tab",
    "Ctrl+C",
    "Slash",
    "Enter",
    "Left arrow",
    "Right arrow",
  ]) {
    const key = keys.getByRole("button", { name, exact: true });
    await key.scrollIntoViewIfNeeded();
    await expect(key).toBeInViewport({ ratio: 1 });
    expect((await key.boundingBox())?.height).toBeGreaterThanOrEqual(43.5);
    await key.tap();
  }
  await expect
    .poll(async () => (await stubState(page)).input)
    .toBe("\x1b\x1b[Z\x1b[A\x1b[B\t\x03/\r\x1b[D\x1b[C");
  await expect(textarea(page)).toBeFocused();
  await expect(fade("left")).toHaveCSS("opacity", "1");
  await expect(fade("right")).toHaveCSS("opacity", "0");
  await fitsTheWidth(page);
});

test("AC2 a key bar key, like a typed key, brings a scrolled-back terminal to the bottom", async ({
  page,
}) => {
  await page.goto(SESSION);
  await connected(page);
  await page.getByTestId("terminal").tap();
  // The stub echoes, so CR (Enter) then LF (Ctrl+J) is a new line.
  for (let i = 0; i < 60; i++) {
    await page.keyboard.insertText(`line ${String(i)}`);
    await page.keyboard.press("Enter");
    await page.keyboard.press("Control+J");
  }
  const rows = page.locator(".xterm-rows");
  await expect(rows).toContainText("line 59");
  const box = await page.getByTestId("terminal").boundingBox();
  await page.mouse.move((box?.x ?? 0) + 50, (box?.y ?? 0) + 50);
  await page.mouse.wheel(0, -5000);
  await expect(rows).not.toContainText("line 59");
  await page
    .getByRole("group", { name: "Terminal keys" })
    .getByRole("button", { name: "Up arrow" })
    .tap();
  await expect(rows).toContainText("line 59");
});

test("AC2 the scroll-back for the caret leaves a pinch-zoom alone", async ({ page }) => {
  await page.goto(SESSION);
  await connected(page);
  const scrolls = await page.evaluate(() => {
    const vv = window.visualViewport;
    if (!vv) throw new Error("no visualViewport");
    let calls = 0;
    window.scrollTo = () => {
      calls += 1;
    };
    const resize = (scale: number): number => {
      Object.defineProperty(vv, "scale", { value: scale, configurable: true });
      vv.dispatchEvent(new Event("resize"));
      return calls;
    };
    return [resize(2), resize(1)];
  });
  expect(scrolls).toEqual([0, 1]);
});

test("AC1 the tabs are ARIA tabs: one Tab stop, arrows, Home and End", async ({ page }) => {
  await page.goto(SESSION);
  await connected(page);
  const tab = (name: string) => page.getByRole("tab", { name });
  await expect(tab("Terminal")).toHaveAttribute("tabindex", "0");
  await expect(tab("Feed")).toHaveAttribute("tabindex", "-1");
  await expect(page.getByRole("tabpanel", { name: "Terminal" })).toBeVisible();
  await tab("Terminal").focus();
  for (const [key, selected] of [
    ["ArrowRight", "Feed"],
    ["ArrowRight", "Terminal"],
    ["End", "Feed"],
    ["Home", "Terminal"],
    ["ArrowLeft", "Feed"],
  ] as const) {
    await page.keyboard.press(key);
    await expect(tab(selected)).toBeFocused();
    await expect(tab(selected)).toHaveAttribute("aria-selected", "true");
    await expect(tab(selected)).toHaveAttribute("tabindex", "0");
    await expect(page.getByRole("tabpanel", { name: selected })).toBeVisible();
  }
});

test("AC2 the terminal refits when the keyboard shrinks the visible viewport", async ({ page }) => {
  await page.goto(SESSION);
  await connected(page);
  const before = (await stubState(page)).lastResize?.rows ?? 0;
  // A soft keyboard opening: the visual viewport shrinks, the layout viewport doesn't.
  await page.evaluate(() => {
    const vv = window.visualViewport;
    if (!vv) throw new Error("no visualViewport");
    Object.defineProperty(vv, "height", { value: vv.height / 2 });
    vv.dispatchEvent(new Event("resize"));
  });
  await expect.poll(async () => (await stubState(page)).lastResize?.rows ?? 0).toBeLessThan(before);
  await expect(page.getByRole("group", { name: "Terminal keys" })).toBeInViewport();
  await fitsTheWidth(page);
  // Typing never scrolls the page: the header stays out of the way.
  await page.getByTestId("terminal").tap();
  for (let i = 0; i < 40; i++) await page.keyboard.type(`line ${String(i)}\r`);
  await expect(page.locator(".xterm-rows")).toContainText("line 39");
  await expect(page.getByRole("heading", { level: 1 })).toBeHidden();
  expect(await page.evaluate(() => [scrollX, scrollY])).toEqual([0, 0]);
});

test("AC3 the loop by touch: open a lane, /login in its terminal, watch the feed, its PR, back", async ({
  page,
}) => {
  await page.goto("/");
  await row(page, "RAN-101").getByRole("link", { name: "Cart totals round half-even" }).tap();
  await expect(page.getByRole("heading", { name: "Cart totals round half-even" })).toBeVisible();
  await connected(page);
  await page.getByTestId("terminal").tap();
  await page.keyboard.type("/login");
  await page
    .getByRole("group", { name: "Terminal keys" })
    .getByRole("button", { name: "Enter" })
    .tap();
  await expect.poll(async () => (await stubState(page)).input).toBe("/login\r");
  await expect(page.locator(".xterm-rows")).toContainText("/login");

  await page.getByRole("tab", { name: "Feed" }).tap();
  await expect.poll(async () => (await stubState(page)).watching).toBe(1);
  await stub(page, "emit", { sessionId: FIRST, state: DesiredState.WARM });
  await expect(page.getByRole("region", { name: "Feed" })).toContainText("State changed to warm");
  // The runner opens the PR: its link shows beside the ticket's, a thumb's size. (Not tapped: it
  // leaves for github.com.)
  const url = "https://github.com/acme/storefront/pull/7";
  await stub(page, "report", { sessionId: FIRST, phase: Phase.RUNNING, pullRequestUrl: url });
  const pr = page.locator("header").getByRole("link", { name: /#7 pull request/ });
  await expect(pr).toHaveAttribute("href", url);
  await expect(pr).toHaveAttribute("target", "_blank");
  await touchTargets(page);

  await page.getByRole("link", { name: "Sessions" }).first().tap();
  await expect(row(page, "RAN-102")).toBeVisible();
});

test("RAN-63 waiting for you fits the phone, on the list and in the session view", async ({
  page,
}) => {
  await page.goto("/");
  await expect.poll(async () => (await stubState(page)).watching).toBe(1);
  const question =
    "Please sign in to Claude: open this lane's terminal, type /login, and answer Yes when Claude asks whether you trust the folder. The lane starts by itself once you're signed in.";
  await stub(page, "agent", { sessionId: FIRST, question });
  await expect(row(page, "RAN-101")).toContainText("Waiting for you");
  await fitsTheWidth(page);
  await page.goto(SESSION);
  await expect(page.locator("header")).toContainText(`Waiting for you: ${question}`);
  // Once the terminal has fitted itself to the screen it is left with and the view has settled.
  await expect(() => fitsTheWidth(page)).toPass({ timeout: 5_000 });
});
