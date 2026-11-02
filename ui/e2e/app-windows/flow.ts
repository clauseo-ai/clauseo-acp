// The steps every spec takes in the room: open a matter, send a prompt, wait for the turn to end.
// Built on selectors.ts only.

import { expect } from "@playwright/test";
import type { StopReason, Window } from "./selectors.ts";

/**
 * Unfolds the rail if it is folded behind Matters (below 1200 px; opening a matter folds it
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
 * Makes a matter through the rail's New matter form and waits until it is open. `existing`
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
    // Create waits until the form names the picked folder: pressed while the picker still runs,
    // it would make a new folder instead (autoplay.ts waits the same way).
    const folder = async () =>
      (await w.folderLine.count()) === 0 ? null : await w.folderLine.textContent();
    const before = await folder();
    await w.matterExisting.click();
    await expect.poll(folder).not.toBe(before);
    await expect(w.matterExisting).toBeEnabled();
  }
  await w.createMatter.click();
  await expect(w.matter).toHaveText(name);
  await expect(w.createMatter).toBeHidden();
}

/**
 * Opens the matter folder E2E_MATTER_DIR (the e2e build's pick_folder returns it) as the matter
 * `name`: New matter, the name, Change, Create the matter; the toolbar title then names the
 * matter, and the composer is enabled, so a spec can type at once.
 */
export async function openMatter(w: Window, name = "Matter"): Promise<void> {
  await createRoomMatter(w, name, { existing: true });
  await expect(w.composer).toBeEnabled();
}

/** Presses the rail row of the matter named `name` and waits until it is open. */
export async function moveTo(w: Window, name: string): Promise<void> {
  await showRail(w);
  await w.matterRow(name).click();
  await expect(w.matter).toHaveText(name);
}

/**
 * Presses New conversation under the open matter and waits for its draft: a current row that
 * says "New conversation" (English build words), and an empty transcript. Nothing reaches the
 * host until the first send, which creates the thread and then names it from that message.
 */
export async function newConversation(w: Window): Promise<void> {
  await showRail(w);
  await w.newConversation.click();
  const draft = w.conversationRow("New conversation");
  await expect(draft).toHaveAttribute("aria-current", "true");
  await expect(w.userMessages).toHaveCount(0);
  await expect(w.composer).toBeEnabled();
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
