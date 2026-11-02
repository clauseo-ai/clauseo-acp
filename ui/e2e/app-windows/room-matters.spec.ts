// The room's rail: opening the e2e folder as a matter, two matters and moving between them,
// finding one, and "Answering." on a matter left while its turn runs.
//
// The e2e build's pick_folder always returns E2E_MATTER_DIR. A folder can be one matter only and a
// matter may not sit inside another (crates/matters/src/refuse.rs), so a test with two matters
// makes both with the new-folder source: Create the matter without Change picks the parent
// (E2E_MATTER_DIR) and the host makes `<parent>/<name>`, two sibling folders.

import { expect } from "@playwright/test";
import { test } from "./fixture.ts";
import { createRoomMatter, expectTurnEnded, moveTo, sendPrompt, showRail } from "./flow.ts";
import { window, type Window } from "./selectors.ts";

const FIRST = "Lease review";
const SECOND = "Share purchase";

/** Makes FIRST then SECOND as new folders; SECOND is open after. */
async function twoMatters(w: Window): Promise<void> {
  await createRoomMatter(w, FIRST, { existing: false });
  await createRoomMatter(w, SECOND, { existing: false });
}

/** The row of `name` is the rail's current row, and the other's is not. */
async function expectCurrent(w: Window, name: string, other: string): Promise<void> {
  await expect(w.matterRow(name)).toHaveAttribute("aria-current", "page");
  await expect(w.matterRow(other)).not.toHaveAttribute("aria-current", "page");
}

test("an existing folder becomes a matter and opens", async ({ app }) => {
  const w = window(app.page);
  await createRoomMatter(w, FIRST, { existing: true });
  await expect(w.matterRows).toHaveCount(1);
  await expect(w.matterRow(FIRST)).toHaveAttribute("aria-current", "page");
});

test("two matters, and moving between them", async ({ app }) => {
  const w = window(app.page);
  await twoMatters(w);
  await expect(w.matterRows).toHaveCount(2);
  await expectCurrent(w, SECOND, FIRST);

  await moveTo(w, FIRST);
  await expectCurrent(w, FIRST, SECOND);

  await moveTo(w, SECOND);
  await expectCurrent(w, SECOND, FIRST);
});

test("find filters the rows and Enter opens the first", async ({ app }) => {
  const w = window(app.page);
  await twoMatters(w);
  await showRail(w);
  await w.findMatter.fill("lease");
  await expect(w.matterRows).toHaveCount(1);
  await expect(w.matterRows).toContainText(FIRST);
  await w.findMatter.press("Enter");
  await expect(w.matter).toHaveText(FIRST);
  await expect(w.matterRow(FIRST)).toHaveAttribute("aria-current", "page");
});

test.describe("a matter left while its turn runs", () => {
  test.use({ scenario: "cancel-unanswered" });

  // Moved to U3's done-when: the room has no composer until U3's work fills the centre slot
  // (composer, send, cancel). U3 turns this into `test`.
  test.fixme("says Answering. on its row, and its turn can still be cancelled", async ({ app }) => {
    const w = window(app.page);
    await twoMatters(w);
    await moveTo(w, FIRST);
    // fake_agent's cancel-unanswered streams "The clause" and then never ends the turn.
    await sendPrompt(w, "Summarise the indemnity clause");
    await expect(w.agentMessages.last()).toHaveText("The clause");
    await expect(w.cancel).toBeVisible();

    await moveTo(w, SECOND);
    // The room keeps the left thread's header and shows its status on the row.
    const left = w.matterRow(FIRST);
    await expect(left).toHaveAttribute("data-state", "working");
    await expect(left).toContainText("Answering.");

    await moveTo(w, FIRST);
    await w.cancel.click();
    await expectTurnEnded(w, "cancelled");
    await expect(w.matterRow(FIRST)).not.toHaveAttribute("data-state", "working");
  });
});
