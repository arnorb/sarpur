// Criterion 17: posters, the download dialog and Follow by keyboard, Escape
// closing dialogs, no horizontal scroll at 320 px, and text contrast of at
// least 4.5:1.
import AxeBuilder from "@axe-core/playwright";
import { expect, type Page, test } from "@playwright/test";

// A 1×1 PNG, served for every poster so the tests don't reach RÚV.
const PIXEL = Buffer.from(
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNkYAAAAAYAAjCB0C8AAAAASUVORK5CYII=",
  "base64",
);

test.beforeEach(async ({ page }) => {
  await page.route("**/api/poster?**", (route) => route.fulfill({ contentType: "image/png", body: PIXEL }));
});

async function open(page: Page, path = "/") {
  await page.goto(path);
  await expect(page.getByRole("list", { name: "Series" }).or(page.getByRole("heading", { level: 1 }))).toBeVisible();
}

/** Tabs until `target` has focus (at most 60 presses). */
async function tabTo(page: Page, target: ReturnType<Page["getByRole"]>) {
  for (let i = 0; i < 60; i++) {
    if (await target.evaluate((el) => el === document.activeElement)) return;
    await page.keyboard.press("Tab");
  }
  throw new Error("never reached the element by Tab");
}

test("posters show each series with its image", async ({ page }) => {
  await open(page);
  const grid = page.getByRole("list", { name: "Series" });
  await expect(grid.getByRole("listitem")).toHaveCount(3);
  await expect(grid.getByRole("link", { name: "Ófærð" })).toBeVisible();
  const image = grid.getByRole("listitem").first().locator("img");
  await expect(image).toHaveAttribute("src", /\/api\/poster\?url=https%3A%2F%2Fmyndir\.ruv\.is/);
  await expect.poll(() => image.evaluate((img: HTMLImageElement) => img.naturalWidth)).toBe(1);
});

test("download dialog by keyboard; Escape closes it and focus returns", async ({ page }) => {
  await open(page);
  await tabTo(page, page.getByRole("link", { name: "Ófærð" }));
  await page.keyboard.press("Enter");
  await expect(page.getByRole("heading", { name: "Ófærð" })).toBeVisible();

  const download = page.getByRole("button", { name: "Download: Ófærð s01e03" });
  await tabTo(page, download);
  await page.keyboard.press("Enter");
  const dialog = page.getByRole("dialog", { name: "Download to this computer" });
  await expect(dialog).toBeVisible();
  await expect(dialog.getByRole("checkbox", { name: /Icelandic/ })).toBeChecked();
  // Focus stays inside the modal.
  await page.keyboard.press("Tab");
  expect(await dialog.evaluate((d) => d.contains(document.activeElement))).toBe(true);
  await page.keyboard.press("Escape");
  await expect(dialog).toBeHidden();
  await expect(download).toBeFocused();

  // Queue a library download by keyboard; the dry run completes it.
  await tabTo(page, page.getByRole("button", { name: "Library: Ófærð s01e03" }));
  await page.keyboard.press("Enter");
  const library = page.getByRole("dialog", { name: "Download to library" });
  await tabTo(page, library.getByRole("button", { name: "Download to library" }));
  await page.keyboard.press("Enter");
  // The queue starts collapsed so it doesn't cover the page; open it.
  await page.getByRole("button", { name: /^Downloads/ }).click();
  const queue = page.getByRole("complementary", { name: "Download queue" });
  await expect(page.getByRole("button", { name: "Close the download queue" })).toBeFocused();
  await expect(queue.getByText("Completed", { exact: true })).toBeVisible();
});

test("Follow by keyboard from a poster, then on the Following page", async ({ page }) => {
  await open(page);
  const follow = page.getByRole("button", { name: "Follow Fréttir" });
  await tabTo(page, follow);
  // The follow bar shows when the poster has keyboard focus.
  await expect(follow).toBeVisible();
  await page.keyboard.press("Enter");
  await expect(follow).toHaveAttribute("aria-pressed", "true");
  // Focused, a followed button says what Enter would do.
  await expect(follow).toContainText("Unfollow");

  await page.getByRole("link", { name: "Following" }).click();
  await expect(page.getByRole("heading", { name: "Following", level: 1 })).toBeFocused();
  await expect(page).toHaveTitle("Following – RÚV Sarpur");
  await expect(page.getByRole("heading", { name: "Fréttir" })).toBeVisible();
  await page.getByRole("button", { name: "Unfollow" }).click();
  await expect(page.getByText("You don’t follow anything yet.")).toBeVisible();
  // Unfollowing can be undone.
  await page.getByRole("button", { name: "Undo" }).click();
  await expect(page.getByRole("heading", { name: "Fréttir" })).toBeVisible();
  await page.getByRole("button", { name: "Unfollow" }).click();
  await expect(page.getByText("You don’t follow anything yet.")).toBeVisible();
});

test("the search field shows keyboard focus", async ({ page }) => {
  await open(page);
  await page.getByRole("searchbox", { name: "Search by title" }).or(page.getByRole("textbox", { name: "Search by title" })).focus();
  const ring = await page.getByRole("search").evaluate((form) => getComputedStyle(form).boxShadow);
  expect(ring).not.toBe("none");
});

test("Settings keeps unsaved changes and shows results inside the dialog", async ({ page }) => {
  await open(page);
  await page.getByRole("button", { name: "Settings" }).click();
  const dialog = page.getByRole("dialog", { name: "Settings" });
  await dialog.getByRole("button", { name: "Automatic downloads", exact: true }).click();
  const interval = dialog.getByRole("spinbutton", { name: "Check every (minutes)" });
  await expect(dialog.getByRole("button", { name: "Saved" })).toBeVisible();
  // The typed number stays as typed (no clamping per keystroke)…
  await interval.fill("");
  await interval.pressSequentially("15");
  await expect(interval).toHaveValue("15");
  await interval.fill("3");
  await interval.blur();
  // …and is corrected only on leaving the field.
  await expect(interval).toHaveValue("5");
  const auto = dialog.getByRole("checkbox", { name: /automatically/ });
  await auto.check();
  await dialog.getByRole("button", { name: "Library", exact: true }).click();
  await dialog.getByRole("button", { name: "Automatic downloads", exact: true }).click();
  await expect(auto).toBeChecked();
  await expect(dialog.getByRole("button", { name: "Save changes" })).toBeVisible();

  // An error from an action in Settings is shown in the dialog, not behind it.
  await dialog.getByRole("button", { name: "Import", exact: true }).click();
  await dialog.getByLabel("ruvsarpur’s folder").fill("/no/such/folder");
  await dialog.getByRole("button", { name: "Preview" }).click();
  // The visible notice (a span), not its screen-reader copy in the live region.
  await expect(dialog.locator("span").filter({ hasText: /Couldn't preview the import/ })).toBeVisible();
});

test("Escape closes Settings", async ({ page }) => {
  await open(page);
  await page.getByRole("button", { name: "Settings" }).click();
  const dialog = page.getByRole("dialog", { name: "Settings" });
  await expect(dialog).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(dialog).toBeHidden();
});

test.describe("at 320 px wide", () => {
  test.use({ viewport: { width: 320, height: 640 } });
  for (const path of ["/", "/title/100", "/title/400", "/following"]) {
    test(`no horizontal scroll on ${path}`, async ({ page }) => {
      await open(page, path);
      // Also with the list view and the download queue showing.
      if (path === "/") await page.getByRole("button", { name: "List" }).click();
      const overflow = await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
      expect(overflow).toBeLessThanOrEqual(0);
    });
  }
  test("no horizontal scroll with Settings open", async ({ page }) => {
    await open(page);
    await page.getByRole("button", { name: "Settings" }).click();
    for (const section of ["This browser", "Library", "Import"]) {
      await page.getByRole("button", { name: section, exact: true }).click();
      const dialog = page.getByRole("dialog", { name: "Settings" });
      const overflow = await dialog.evaluate((d) => d.scrollWidth - d.clientWidth);
      expect(overflow, section).toBeLessThanOrEqual(0);
    }
  });
});

test.describe("text contrast is at least 4.5:1", () => {
  const check = async (page: Page) => {
    // Let colour transitions (150 ms) finish first.
    await page.waitForTimeout(300);
    const results = await new AxeBuilder({ page }).withRules(["color-contrast"]).analyze();
    const failures = results.violations.flatMap((v) => v.nodes.map((n) => `${n.target.join(" ")}: ${n.failureSummary}`));
    expect(failures).toEqual([]);
  };
  test("browse (posters and list)", async ({ page }) => {
    await open(page);
    await check(page);
    await page.getByRole("button", { name: "List" }).click();
    await check(page);
  });
  test("series page and download dialog", async ({ page }) => {
    await open(page, "/title/100");
    await check(page);
    await page.getByRole("button", { name: "Download: Ófærð s01e03" }).click();
    await check(page);
  });
  test("following and settings", async ({ page }) => {
    await open(page);
    await page.getByRole("button", { name: "Follow Ófærð" }).focus();
    await page.keyboard.press("Enter");
    await page.getByRole("link", { name: "Following" }).click();
    await check(page);
    await page.getByRole("button", { name: "Settings" }).click();
    for (const section of ["This browser", "Automatic downloads", "Library", "Plex", "Import"]) {
      await page.getByRole("button", { name: section, exact: true }).click();
      await check(page);
    }
  });
});
