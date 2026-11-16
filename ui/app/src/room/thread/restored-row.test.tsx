// @vitest-environment happy-dom
// A restore made from Versions while the conversation was open: the host records it on the conversation's stream as a
// version_restored marker, which the thread shows as a quiet line, saying whether it undid a restore; any other marker
// still shows nothing.
import { th } from "@clauseo/components/lib/words-thread.ts";
import { afterEach, expect, test } from "vitest";
import { agentText, reset, TURN, userMessage, uuid } from "../../testing/stream-script.ts";
import type { MarkerKind, ViewEntry } from "../../transport/protocol.ts";
import { mountRoom, type Harness } from "./harness.ts";

let h: Harness | null = null;
afterEach(async () => {
  await h?.unmount();
  h = null;
  document.body.replaceChildren();
});

const marker = (order: number, kind: MarkerKind): ViewEntry => ({
  marker: { order, event: uuid(order, 9), ts: Date.UTC(2026, 10, 2, 12, order), kind },
});

test("the conversation shows a restore and the undo of a restore as quiet lines, and other markers as nothing", async () => {
  h = await mountRoom();
  h.deliver([
    reset([
      userMessage(1, 1, "Draft the reply"),
      agentText(2, 1, "Done."),
      marker(3, { version_restored: { from: "cp2", checkpoint: "cp4", scope: "matter" } }),
      marker(4, {
        version_restored: { from: "cp3", checkpoint: "cp5", scope: "matter", undoes: "cp4" },
      }),
      marker(5, {
        step_undone: { turn: TURN, checkpoint: "cp6", restored: ["Lost.md"], kept: [] },
      }),
    ]),
  ]);
  const rows = h.all("marker-restored");
  expect(rows.map((r) => r.textContent)).toEqual([
    th("en", "You restored an earlier version of the matter."),
    th("en", "You undid the restore."),
  ]);
  expect(rows.map((r) => r.textContent)).toEqual([
    "You restored an earlier version of the matter.",
    "You undid the restore.",
  ]);
  // the step's undo marker draws nothing of its own
  expect(document.body.querySelectorAll("[data-entry-id^='marker:']")).toHaveLength(2);
});
