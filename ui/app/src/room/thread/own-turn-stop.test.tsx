// @vitest-environment happy-dom
// A turn Claude starts on its own (after a background task, with no question of the lawyer's): the host's view says
// `starting` as the turn opens, before its checkpoint and its start, and Stop stands in Ask's place from that moment;
// pressing it sends cancel. The turn then ends before it started (no step), and Ask is back. A prompt queued while
// a turn is still starting leaves the header `starting`: its queued row shows with Stop still in Ask's place.
import { afterEach, expect, test } from "vitest";
import {
  agentText,
  header,
  reset,
  step,
  THREAD,
  upsert,
  userMessage,
  uuid,
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

test("a prompt queued while a turn is starting: its row shows, and Stop stays", async () => {
  h = await mountRoom();
  h.deliver([
    reset(
      [userMessage(5, 1, "Run the build."), agentText(6, 1, "Started it."), step(7)],
      header("idle"),
    ),
  ]);
  // the own turn opens, then the lawyer's prompt queues behind it; the header stays starting
  h.deliver([upsert(header("starting"))]);
  h.deliver([
    upsert({
      user_message: {
        ...userMessage(8, 2, "Then the summary.").user_message!,
        status: "queued",
        turn: uuid(102, 4),
      },
    }),
  ]);
  const rows = h.all("queued");
  expect(rows).toHaveLength(1);
  expect(rows[0]?.textContent).toContain("Then the summary.");
  expect(h.one("queued-edit")).not.toBeNull();
  expect(h.one("queued-remove")).not.toBeNull();
  const stop = h.one("cancel") as HTMLButtonElement | null;
  expect(stop).not.toBeNull();
  expect(stop?.disabled).toBe(false);
  expect(h.one("send")).toBeNull();
});
