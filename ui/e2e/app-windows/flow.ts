// The steps every spec takes in the room: open a matter, send a prompt, wait for the turn to end.
// Built on selectors.ts only.

import { fileURLToPath } from "node:url";
import { expect } from "@playwright/test";
import type { StopReason, Window } from "./selectors.ts";

/**
 * Unfolds the rail if it is folded behind Matters (below 1200 px; opening a matter folds it
 * again), so New matter, the find field and the rows can be pressed. From 1200 px the rail is a
 * column of its own and Matters isn't drawn at all.
 */
export async function showRail(w: Window): Promise<void> {
  // The room is hidden while it starts (shell.css), so first wait until New matter or Matters
  // shows: a check before then read every width as folded and pressed a Matters that a wide
  // window never draws. Only a shown one counts: Matters comes first in the page, and from
  // 1200 px it is there but not drawn.
  await expect(w.newMatter.or(w.railToggle).filter({ visible: true }).first()).toBeVisible();
  if (!(await w.newMatter.isVisible())) {
    await w.railToggle.click();
  }
  await expect(w.newMatter).toBeVisible();
}

/**
 * Moves the pointer off the rail, to the window's right edge. While the pointer is over the rail,
 * each row keeps the group it stands in (rail-triage.ts), and from 1200 px the rail stays open
 * after a press, so a spec that reads the groups steps off it first.
 */
export async function offRail(w: Window): Promise<void> {
  const page = w.newMatter.page();
  const view = await page.evaluate(() => ({ width: innerWidth, height: innerHeight }));
  await page.mouse.move(view.width - 8, Math.round(view.height / 2));
}

/**
 * Makes a matter through the rail's New matter form and waits until it is open. `existing`
 * presses Choose folder… (pick_folder returns E2E_MATTER_DIR, which becomes the matter); without
 * it, Use a new folder chooses a new folder named after the matter in the host's default location
 * (the scratch matters folder, settings.json's `matters.default_location`; the desktop reads it with
 * get_default_location), which the host makes. A folder can be one matter only, and a matter may not
 * sit inside another, so a test that makes several matters makes them all without `existing`.
 */
export async function createRoomMatter(
  w: Window,
  name: string,
  { existing }: { existing: boolean },
): Promise<void> {
  await showRail(w);
  await w.newMatter.click();
  await w.matterName.fill(name);
  if (!existing) await w.matterMade.click();
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
 * `name`: New matter, the name, Choose folder…, Create matter; the toolbar title then names the
 * matter, and the composer is visible and enabled, so a spec can type at once; a hidden composer
 * (the first-run sign-in gate replaces it) fails here at once, since a hidden textarea still reads
 * as enabled. A spec whose plan starts signed out by design opens it with {@link openGatedMatter}.
 */
export async function openMatter(w: Window, name = "Matter"): Promise<void> {
  await createRoomMatter(w, name, { existing: true });
  await expect(w.composer).toBeVisible();
  await expect(w.composer).toBeEnabled();
}

/**
 * Opens the matter as {@link openMatter} does, for a spec whose plan starts signed out
 * (signin-fake): the first run's sign-in card stands in the composer's place, so this waits for
 * the card, visible, and for the composer, hidden.
 */
export async function openGatedMatter(w: Window, name = "Matter"): Promise<void> {
  await createRoomMatter(w, name, { existing: true });
  await expect(w.signIn).toBeVisible();
  await expect(w.composer).toBeHidden();
}

/**
 * `claude-files-one-turn` with the first read answered (scenarios/claude-files-one-turn.jsonl), as
 * a path, so fake_agent loads it from its file: the spec for a matter that starts with papers.
 */
export const FILES_SCENARIO = fileURLToPath(
  new URL("scenarios/claude-files-one-turn.jsonl", import.meta.url),
);

/** What the scenarios in scenarios/ answer the first-read prompt with. */
export const FIRST_READ_ANSWER = "A plain summary of the papers.";

/**
 * A matter made on the first screen that starts with papers is offered its first read once the
 * plan is signed in (UD2, D-015; ui/app/src/room/first-run/first-run.tsx): "Read the papers" stands
 * under the new conversation's line. Nothing sends the read by itself, so this presses that button,
 * which makes a "First read" conversation and sends the first-read prompt in it. Waits until that
 * read is answered (the scenarios in scenarios/ answer it with {@link FIRST_READ_ANSWER} and change
 * no file), then opens a new conversation, so the spec's own prompt starts a thread of its own.
 */
export async function finishFirstRead(w: Window): Promise<void> {
  await expect(w.firstReadOffer).toBeVisible();
  await w.firstReadOffer.click();
  await expect(w.firstRead).toHaveAttribute("data-state", "read");
  await expect(w.agentMessages.last()).toHaveText(FIRST_READ_ANSWER);
  await expectTurnEnded(w, "end_turn");
  await newConversation(w);
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
  await expect(w.composer).toBeVisible();
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
