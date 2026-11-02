// Settings in a window of its own (U11, H14). The rail's Settings opens the settings window, and a
// second press brings it forward, so there is still one. Choosing Night there turns the main
// window to night at once, with nothing reloaded: the composer keeps the words typed into it.

import { expect } from "@playwright/test";
import { pagesAt, test, windowAt } from "./fixture.ts";
import { openMatter, showRail } from "./flow.ts";
import { SETTINGS_PAGE, settingsWindow, window } from "./selectors.ts";

/** Words typed and never sent: they must outlive the change of theme. */
const DRAFT = "Check the limitation period for the recovery suit";

test("Settings opens one window, and Night there turns the room to night with its draft kept", async ({
  app,
}) => {
  const w = window(app.page);
  await openMatter(w);
  await w.composer.fill(DRAFT);

  await showRail(w);
  expect(pagesAt(app, SETTINGS_PAGE)).toHaveLength(0);
  await w.openSettings.click();
  const settings = await windowAt(app, SETTINGS_PAGE);
  const s = settingsWindow(settings);
  await expect(s.appearance).toBeVisible();

  // a second press focuses the window it opened: still one settings page
  await showRail(w);
  await w.openSettings.click();
  await expect(settings.locator("html")).toBeAttached();
  await expect.poll(() => pagesAt(app, SETTINGS_PAGE).length).toBe(1);
  expect(pagesAt(app, SETTINGS_PAGE)[0]).toBe(settings);

  await s.appearance.click();
  await expect(s.appearance).toHaveAttribute("aria-current", "page");
  await s.theme("Night").click();
  await expect(s.theme("Night")).toHaveAttribute("aria-pressed", "true");

  // both windows show night; the main window was not reloaded
  await expect(settings.locator("html")).toHaveAttribute("data-theme", "night");
  await expect(app.page.locator("html")).toHaveAttribute("data-theme", "night");
  await expect(w.composer).toHaveValue(DRAFT);
});
