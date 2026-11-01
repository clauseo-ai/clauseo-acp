// The steps every spec takes: open a matter, send a prompt, wait for the turn to end. Built on
// selectors.ts only. `E2E_WINDOW=room` says the e2e build is the room's
// (`VITE_CLAUSEO_WINDOW=room`); unset, it is R12's internal window.

import { expect } from "@playwright/test";
import type { StopReason, Window } from "./selectors.ts";

/** Whether the e2e build under test is the room's. */
export const isRoom = (): boolean => process.env["E2E_WINDOW"] === "room";

/**
 * Room: unfolds the rail if it is folded behind Matters (below 1200 px; opening a matter folds it
 * again), so New matter, the find field and the rows can be pressed.
 */
export async function showRail(w: Window): Promise<void> {
  // New matter is in the page from the first render, folded or not.
  await expect(w.newMatter).toBeAttached();
  if (!(await w.newMatter.isVisible())) {
    await w.railToggle.click();
  }
  await expect(w.newMatter).toBeVisible();
}

/**
 * Room: makes a matter through the rail's New matter form and waits until it is open. `existing`
 * presses Change first (pick_folder returns E2E_MATTER_DIR, which becomes the matter); without
 * it, Create picks the parent (E2E_MATTER_DIR again) and the host makes `<parent>/<name>`. A
 * folder can be one matter only, and a matter may not sit inside another, so a test that makes
 * several matters makes them all without `existing`.
 */
export async function createRoomMatter(
  w: Window,
  name: string,
  { existing }: { existing: boolean },
): Promise<void> {
  await showRail(w);
  await w.newMatter.click();
  await w.matterName.fill(name);
  if (existing) {
    await w.matterExisting.click();
  }
  await w.createMatter.click();
  await expect(w.matter).toHaveText(name);
  await expect(w.createMatter).toBeHidden();
}

/**
 * Opens the matter folder E2E_MATTER_DIR (the e2e build's pick_folder returns it).
 *
 * Internal window: New matter under the prefilled name; the window then makes a thread, which
 * enables the composer (it shows but stays disabled while no thread is open).
 *
 * Room (`E2E_WINDOW=room`): New matter, the name, Change, Create the matter; the toolbar title
 * then names the matter, and the composer is enabled, as in the internal window: today's specs type
 * at once. The room has a composer once U3's work stands in the centre slot.
 */
export async function openMatter(w: Window, name = "Matter"): Promise<void> {
  if (isRoom()) {
    await createRoomMatter(w, name, { existing: true });
    await expect(w.composer).toBeEnabled();
    return;
  }
  await w.matterName.fill(name);
  await w.newMatter.click();
  await expect(w.matter).toHaveText(name);
  await expect(w.composer).toBeEnabled();
}

/** Room: presses the rail row of the matter named `name` and waits until it is open. */
export async function moveTo(w: Window, name: string): Promise<void> {
  await showRail(w);
  await w.matterRow(name).click();
  await expect(w.matter).toHaveText(name);
}

/** Types `text` and sends it; the user's message shows in the transcript. */
export async function sendPrompt(w: Window, text: string): Promise<void> {
  await w.composer.fill(text);
  await w.send.click();
  await expect(w.userMessages.last()).toContainText(text);
}

/** The last turn ended with `reason`: its stop line shows and Cancel is gone. */
export async function expectTurnEnded(w: Window, reason: StopReason): Promise<void> {
  await expect(w.turnStopped.last()).toHaveAttribute("data-stop-reason", reason);
  await expect(w.cancel).toBeHidden();
}
