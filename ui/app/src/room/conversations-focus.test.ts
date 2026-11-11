// @vitest-environment happy-dom
// D-037 (b): an answer that ended while the window lacked focus (the lawyer in Word) is marked seen once the window comes
// back to its open conversation (its `focus` event, or the document shown again), once, through H6's mark_seen; an
// open thread already seen, or a window still unfocused, marks nothing.
import { expect, test } from "vitest";
import { A, info, next, rig, T1, T2 } from "./conversations-rig.ts";

const UNSEEN = { seen: false, last_stop: { reason: "end_turn" as const, at: 5 } };
const appFrame = (threads: ReturnType<typeof info>[]) => ({
  threads_changed: { matter: A.matter, threads, removed: [], all: false },
});

test("the window coming back marks the open thread's unseen answer seen, once", async () => {
  let focused = false;
  const { talks, transport, settle, marks } = await rig(
    next(() => [info(T1), info(T2)]),
    () => focused,
  );
  expect(talks.view().thread).toBe(T2);
  // its turn ended while the lawyer was in Word: the host says it waits unseen
  transport.appEvent(appFrame([info(T2, UNSEEN)]));
  await settle();
  expect(talks.view().rows.find((r) => r.thread === T2)?.seen).toBe(false);
  expect(marks()).toEqual([]);
  // back in the window
  focused = true;
  window.dispatchEvent(new FocusEvent("focus"));
  expect(marks()).toEqual([{ mark_seen: { thread: T2 } }]);
  expect(talks.view().rows.find((r) => r.thread === T2)?.seen).toBe(true);
  // coming back again, or the document shown again, sends no second mark
  window.dispatchEvent(new FocusEvent("focus"));
  document.dispatchEvent(new Event("visibilitychange"));
  expect(marks()).toEqual([{ mark_seen: { thread: T2 } }]);
});

test("the document shown again with focus marks it too; without focus, or with nothing unseen, nothing", async () => {
  let focused = false;
  const { talks, transport, settle, marks } = await rig(
    next(() => [info(T1), info(T2, UNSEEN)]),
    () => focused,
  );
  expect(talks.view().thread).toBe(T2);
  // shown, yet the window still unfocused: nothing
  document.dispatchEvent(new Event("visibilitychange"));
  window.dispatchEvent(new FocusEvent("focus"));
  expect(marks()).toEqual([]);
  focused = true;
  document.dispatchEvent(new Event("visibilitychange"));
  expect(marks()).toEqual([{ mark_seen: { thread: T2 } }]);
  // another thread unseen but not open: the window coming back leaves it be
  transport.appEvent(appFrame([info(T1, UNSEEN)]));
  await settle();
  window.dispatchEvent(new FocusEvent("focus"));
  expect(marks()).toEqual([{ mark_seen: { thread: T2 } }]);
  expect(talks.view().rows.find((r) => r.thread === T1)?.seen).toBe(false);
});
