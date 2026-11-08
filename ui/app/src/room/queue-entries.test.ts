// The queue's picks over a view (queue-entries.ts): the queued messages in their order, their count, and why the queue
// holds; each the same value while it reads the same.
import { expect, test } from "vitest";
import { applyBatch, newView, type LiveView } from "../store/view.ts";
import {
  batch,
  header,
  reset,
  step,
  STREAM,
  upsert,
  userMessage,
  uuid,
} from "../testing/stream-script.ts";
import type { StreamId, ViewEntry, ViewPatchBody } from "../transport/protocol.ts";
import { queueCountPick } from "./queue-entries.ts";
import { heldPick, queuedPick } from "./queue-picks.ts";

const queued = (order: number, n: number, text: string): ViewEntry => ({
  user_message: {
    ...userMessage(order, n, text).user_message!,
    status: "queued",
    turn: uuid(n, 4),
  },
});
const heldHeader: ViewEntry = { header: { ...header("idle").header!, queue_held: true } };

function viewOf(...bodies: ViewPatchBody[]): {
  view: LiveView;
  apply: (...b: ViewPatchBody[]) => void;
} {
  const view = newView(STREAM as StreamId);
  let cursor = 1;
  const apply = (...b: ViewPatchBody[]) => {
    applyBatch(view, batch(cursor, b));
    cursor += b.length;
  };
  apply(...bodies);
  return { view, apply };
}

test("queued messages in their order, with turn, words and file count; sent ones left out", () => {
  const withFile: ViewEntry = {
    user_message: {
      ...queued(6, 3, "See the annexure.").user_message!,
      content: [
        { text: { text: "See the annexure." } },
        { resource_link: { uri: "file:///a.pdf", name: "a.pdf" } },
      ],
    },
  };
  const { view } = viewOf(
    reset([userMessage(1, 1, "Q1"), queued(5, 2, "Second."), withFile], header("running")),
  );
  const pick = queuedPick();
  const q = pick(view);
  expect(q.map((x) => [x.turn, x.text, x.files])).toEqual([
    [uuid(2, 4), "Second.", 0],
    [uuid(3, 4), "See the annexure.", 1],
  ]);
  expect(queueCountPick(view)).toBe(2);
  // the same array while nothing changed
  expect(pick(view)).toBe(q);
});

test("after a restart the message held for the sign-in goes first, the two queued before it behind it (0048, P4b)", () => {
  // The log's order: "Two" and "Three" queued while the first turn ran, then that turn put back as "Again".
  const again: ViewEntry = {
    user_message: { ...queued(9, 4, "Again.").user_message!, at_head: true },
  };
  const { view } = viewOf(reset([queued(5, 2, "Two."), queued(6, 3, "Three."), again], heldHeader));
  expect(queuedPick()(view).map((x) => x.text)).toEqual(["Again.", "Two.", "Three."]);
  // two put back: the latest first, as the host's restore orders them
  const later: ViewEntry = {
    user_message: { ...queued(11, 5, "Later.").user_message!, at_head: true },
  };
  const two = viewOf(reset([queued(5, 2, "Two."), again, later], heldHeader));
  expect(queuedPick()(two.view).map((x) => x.text)).toEqual(["Later.", "Again.", "Two."]);
});

test("an old snapshot with no at_head orders exactly by entry order, as before", () => {
  const { view } = viewOf(
    reset([queued(5, 2, "Two."), queued(6, 3, "Three."), queued(9, 4, "Again.")], heldHeader),
  );
  const q = queuedPick()(view);
  expect(q.map((x) => x.text)).toEqual(["Two.", "Three.", "Again."]);
  expect(q.every((x) => x.entry.at_head === undefined)).toBe(true);
});

test("an edit gives a new array whose untouched messages keep their identity; a message sent leaves", () => {
  const { view, apply } = viewOf(
    reset([queued(5, 2, "Second."), queued(6, 3, "Third.")], header("running")),
  );
  const pick = queuedPick();
  const first = pick(view);
  apply(
    upsert({ user_message: { ...queued(6, 3, "Third, edited.").user_message!, edited: true } }),
  );
  const second = pick(view);
  expect(second).not.toBe(first);
  expect(second[0]).toBe(first[0]);
  expect(second[1]!.text).toBe("Third, edited.");
  apply(upsert({ user_message: { ...queued(5, 2, "Second.").user_message!, status: "sent" } }));
  expect(pick(view).map((x) => x.text)).toEqual(["Third, edited."]);
  expect(queueCountPick(view)).toBe(1);
});

test("held: after Stop the step's time, after a failure its code, after a restart no step to name", () => {
  const pick = heldPick();
  const stopped = viewOf(
    reset([queued(5, 2, "Q"), step(4, { stop: "cancelled", ts: 7000 })], heldHeader),
  );
  const h1 = pick(stopped.view);
  expect(h1).toEqual({ kind: "stopped", at: 7000 });
  expect(pick(stopped.view)).toBe(h1);
  // a turn the agent ended on the lawyer's refusal holds as a Stop does (private #361)
  const refused = viewOf(
    reset([queued(5, 2, "Q"), step(4, { stop: "not_allowed", ts: 7500 })], heldHeader),
  );
  expect(heldPick()(refused.view)).toEqual({ kind: "not_allowed", at: 7500 });
  const failed = viewOf(
    reset(
      [queued(5, 2, "Q"), step(4, { stop: "failed", failure: "agent.exited", ts: 8000 })],
      heldHeader,
    ),
  );
  expect(heldPick()(failed.view)).toEqual({ kind: "failed", at: 8000, code: "agent.exited" });
  const restarted = viewOf(reset([queued(5, 2, "Q")], heldHeader));
  expect(heldPick()(restarted.view)).toEqual({ kind: "held" });
  // a turn that ended well before the restart names no stop either
  const ended = viewOf(reset([step(3), queued(5, 2, "Q")], heldHeader));
  expect(heldPick()(ended.view)).toEqual({ kind: "held" });
});

test("a turn that ended at a limit after an earlier Stop holds with no stop to name", () => {
  const limit: ViewEntry = {
    limit_stop: { order: 8, turn: uuid(9, 4), reason: "plan_window", ts: 9000 },
  };
  const { view } = viewOf(
    reset(
      [
        step(4, { stop: "cancelled", ts: 4000 }),
        userMessage(6, 6, "Q2"),
        limit,
        queued(10, 2, "Q"),
      ],
      heldHeader,
    ),
  );
  expect(heldPick()(view)).toEqual({ kind: "held" });
});

test("not held: null, whatever the steps say", () => {
  const { view, apply } = viewOf(reset([step(4, { stop: "cancelled" })], header("idle")));
  const pick = heldPick();
  expect(pick(view)).toBeNull();
  apply(upsert(heldHeader));
  expect(pick(view)).toEqual({ kind: "stopped", at: 4000 });
  apply(upsert(header("idle")));
  expect(pick(view)).toBeNull();
  expect(pick(undefined)).toBeNull();
  expect(queueCountPick(undefined)).toBe(0);
});
