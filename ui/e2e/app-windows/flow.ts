// The steps every spec takes in the internal window: open the scratch matter, send a prompt,
// wait for the turn to end. Built on selectors.ts only.

import { expect } from "@playwright/test";
import type { StopReason, Window } from "./selectors.ts";

/**
 * Opens the matter folder: New matter under the prefilled name (pick_folder returns
 * E2E_MATTER_DIR in the e2e build); the window then makes a thread, which enables the composer
 * (it shows but stays disabled while no thread is open).
 */
export async function openMatter(w: Window, name = "Matter"): Promise<void> {
  await w.matterName.fill(name);
  await w.newMatter.click();
  await expect(w.matter).toHaveText(name);
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
