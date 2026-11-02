// The permission prompt's keys in the room, under QU.9 (a): Enter, Ctrl with Enter and Escape
// answer the waiting prompt from the composer only while its field is empty; with words in the
// field Enter keeps its field job. fake_agent `permission-kinds` asks four times and takes only allow_once, allow_always,
// reject_once, reject_always in that order (any other answer ends it), so a wrong key fails the
// next step. allow_always passes Rust's native confirm because the fixture sets
// E2E_CONFIRM_ALWAYS=accept. fake_agent `permission-cancelled` leaves its request pending until
// Stop. The open matter's rail row says nothing, so the row is checked from another matter.

import { expect, type Locator } from "@playwright/test";
import { test } from "./fixture.ts";
import {
  createRoomMatter,
  expectTurnEnded,
  moveTo,
  openMatter,
  sendPrompt,
  showRail,
} from "./flow.ts";
import { window, type Window } from "./selectors.ts";

/** The Nth card is waiting, and the one before it, if any, is answered. */
async function waiting(w: Window, index: number): Promise<Locator> {
  await expect(w.permissions).toHaveCount(index + 1);
  const card = w.permissions.nth(index);
  await expect(card).toHaveAttribute("data-state", "pending");
  return card;
}

/** The card is answered with `kind` and keeps no buttons. */
async function answered(card: Locator, kind: string): Promise<void> {
  await expect(card).toHaveAttribute("data-state", "resolved");
  await expect(card).toHaveAttribute("data-answer", kind);
  await expect(card.getByRole("button")).toHaveCount(0);
}

test.describe("permission-kinds", () => {
  test.use({ scenario: "permission-kinds" });

  test("the keys answer from an empty composer, never over its words", async ({ app }) => {
    const w = window(app.page);
    await openMatter(w);
    await sendPrompt(w, "Edit the four clauses");

    await test.step("Enter in the empty composer allows once", async () => {
      const card = await waiting(w, 0);
      await w.composer.focus();
      await expect(w.composer).toHaveValue("");
      await w.composer.press("Enter");
      await answered(card, "allow_once");
    });

    await test.step("with words in the field, Enter does not answer", async () => {
      const card = await waiting(w, 1);
      await w.composer.fill("And clause ten?");
      await w.composer.press("Enter");
      await expect(w.composer).toHaveValue("And clause ten?");
      await expect(card).toHaveAttribute("data-state", "pending");
      await w.composer.fill("");
      await w.composer.press(process.platform === "darwin" ? "Meta+Enter" : "Control+Enter");
      await answered(card, "allow_always");
    });

    await test.step("Escape in the empty composer declines", async () => {
      const card = await waiting(w, 2);
      await w.composer.press("Escape");
      await answered(card, "reject_once");
    });

    await test.step("Don't allow for this matter has no key, only its button", async () => {
      const card = await waiting(w, 3);
      await w.permissionButton(card, "reject_always").click();
      await answered(card, "reject_always");
    });

    await expect(w.agentMessages.last()).toHaveText("Four edits were decided.");
    await expectTurnEnded(w, "end_turn");
  });
});

const ASKING = "Lease review";
const OTHER = "Share purchase";

test.describe("permission-cancelled", () => {
  test.use({ scenario: "permission-cancelled" });

  test("a left matter's row says it waits; Stop leaves the card without buttons", async ({
    app,
  }) => {
    const w = window(app.page);
    // two sibling matters, both new folders under E2E_MATTER_DIR (room-matters.spec.ts says why)
    await createRoomMatter(w, ASKING, { existing: false });
    await sendPrompt(w, "Edit the brief");
    await waiting(w, 0);
    // the open matter's row says nothing (its work is in view): leave it, and its row says so
    await createRoomMatter(w, OTHER, { existing: false });
    await showRail(w);
    await expect(w.matterRow(ASKING)).toContainText("Needs your permission");
    await moveTo(w, ASKING);
    const card = await waiting(w, 0);
    await w.cancel.click();
    await expect(card).not.toHaveAttribute("data-state", "pending");
    await expect(card.getByRole("button")).toHaveCount(0);
    await expectTurnEnded(w, "cancelled");
  });
});
