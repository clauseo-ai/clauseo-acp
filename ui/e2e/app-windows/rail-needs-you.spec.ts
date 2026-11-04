// U16a's Needs you on the real app: fake_agent `claude-permission-kinds` asks its first permission in a second matter;
// with the lawyer in the first matter, the second stands under Needs you with the conversation behind it named beneath
// its status, and a press opens that conversation with the permission waiting. It needs the desktop's matters and app
// parts (the `rail-wire` change), without which the rail has no Needs you.
//
// The e2e build's pick_folder always returns E2E_MATTER_DIR, so both matters are made as new folders beside each other
// (room-matters.spec.ts says why).

import { expect } from "@playwright/test";
import { test } from "./fixture.ts";
import { createRoomMatter, moveTo, sendPrompt, showRail } from "./flow.ts";
import { window } from "./selectors.ts";

test.use({ scenario: "claude-permission-kinds" });

const FIRST = "Lease review";
const SECOND = "Share purchase";

test("a permission in another matter brings it to Needs you, and a press opens it", async ({
  app,
}) => {
  const w = window(app.page);
  await createRoomMatter(w, FIRST, { existing: false });
  await createRoomMatter(w, SECOND, { existing: false });
  await sendPrompt(w, "Edit the four clauses");
  await expect(w.permissions.first()).toHaveAttribute("data-state", "pending");
  // the conversation's title, as its own row under the open matter says it
  await showRail(w);
  const title = w.conversationTitle(w.conversations.first());
  await expect(title).not.toHaveText("New conversation");
  const named = (await title.textContent()) ?? "";

  await moveTo(w, FIRST);
  await showRail(w);
  const row = w.needsYouRow(SECOND);
  await expect(row).toBeVisible();
  await expect(row).toHaveAttribute("data-state", "permission");
  await expect(row).toContainText("Needs your permission");
  await expect(w.matterWhere(row)).toHaveText(named);
  // the matter left stays out of Needs you: nothing in it waits
  await expect(w.needsYouRow(FIRST)).toHaveCount(0);

  await row.click();
  await expect(w.matter).toHaveText(SECOND);
  await expect(w.permissions.first()).toHaveAttribute("data-state", "pending");
  await expect(w.permissionButton(w.permissions.first(), "allow_once")).toBeVisible();
});
