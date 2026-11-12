// @vitest-environment happy-dom
// A turn Claude starts on its own (after a background task, with no question of the lawyer's): the host's view says
// `starting` as the turn opens, before its checkpoint and its start, and Stop stands in Ask's place from that moment;
// pressing it sends cancel. The turn then ends before it started (no step), and Ask is back.
import { afterEach, expect, test } from "vitest";
import {
  agentText,
  header,
  reset,
  step,
  THREAD,
  upsert,
  userMessage,
} from "../../testing/stream-script.ts";
import { mountRoom, type Harness } from "./harness.ts";

let h: Harness | null = null;
afterEach(async () => {
  await h?.unmount();
  h = null;
});

test("an own turn: Stop from its open, with no question, and pressing it sends cancel", async () => {
  h = await mountRoom();
  // the lawyer's last exchange has ended: Ask
  h.deliver([
    reset(
      [userMessage(5, 1, "Run the build."), agentText(6, 1, "Started it."), step(7)],
      header("idle"),
    ),
  ]);
  expect(h.one("cancel")).toBeNull();
  expect(h.one("send")).not.toBeNull();
  // the agent's own turn opens: only the header changes, no question and no output yet
  h.deliver([upsert(header("starting"))]);
  expect(h.one("send")).toBeNull();
  const stop = h.one("cancel") as HTMLButtonElement | null;
  expect(stop).not.toBeNull();
  expect(stop?.disabled).toBe(false);
  await h.click(stop);
  expect(h.sent("cancel")).toEqual([{ thread: THREAD }]);
  // the host ends it before it started: no step, the header settles, Ask is back
  h.deliver([upsert(header("idle"))]);
  expect(h.one("cancel")).toBeNull();
  expect(h.one("send")).not.toBeNull();
});
