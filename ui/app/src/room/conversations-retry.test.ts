// Retry from here (H13) in conversations.ts over FakeTransport: retry_from with the thread and the question, the thread it
// makes listed and opened with the line's facts, and the refusals.
import { expect, test } from "vitest";
import { uuid } from "../testing/stream-script.ts";
import type { FakeHost, FakeNextHost } from "../transport/fake-transport.ts";
import type { ThreadSummary } from "../transport/protocol.ts";
import { rig, T1, T2, thread, threadsOf, today } from "./conversations-rig.ts";

const QUESTION = uuid(1, 5);

/** Today's threads, with H13's stand-in alone: retry_from makes RETRIED, or is refused. */
function retrying(
  threads: ThreadSummary[],
  answer: NonNullable<FakeNextHost["command"]>,
): FakeHost {
  return {
    ...today(() => threads),
    next: {
      parts: ["retry"],
      query: () => {
        throw new Error("no next query here");
      },
      command: answer,
    },
  };
}

const RETRIED = uuid(8, 1);

test("retry: retry_from with the thread and question; the new thread is listed with the source's agent and opens, with the line's facts", async () => {
  const { talks, transport, settle } = await rig(
    retrying(
      [thread(T1, { title: "Lease notice", agent: "codex" }), thread(T2, { title: "Indemnity" })],
      (command) => ("retry_from" in command ? { thread_created: { thread: RETRIED } } : undefined),
    ),
  );
  expect(talks.view().canRetry).toBe(true);
  expect(talks.view().retried).toBeNull();
  talks.open(T1);
  await talks.retry(T1, QUESTION, 1_000);
  await settle();
  expect(transport.only("next_command").map((c) => c.command)).toEqual([
    { retry_from: { thread: T1, message: QUESTION } },
  ]);
  const view = talks.view();
  expect(view.thread).toBe(RETRIED);
  expect(view.open).toBe(RETRIED);
  expect(view.rows.find((r) => r.thread === RETRIED)).toMatchObject({
    key: RETRIED,
    agent: "codex",
  });
  expect(view.retried).toEqual({ from: T1, title: "Lease notice", at: 1_000 });
  // back to the first: no line there
  talks.open(T1);
  expect(talks.view().retried).toBeNull();
});

test("retry from the thread that opened first: the line names that conversation by the key the room gave it", async () => {
  const { talks } = await rig(
    retrying([thread(T1, { title: "Lease notice" })], () => ({
      thread_created: { thread: RETRIED },
    })),
  );
  const first = talks.view().open;
  expect(first).not.toBe(T1);
  await talks.retry(T1, QUESTION, 7);
  expect(talks.view().retried).toEqual({ from: first, title: "Lease notice", at: 7 });
});

test("without H13's part canRetry is false and retry rejects, sending nothing", async () => {
  const { talks, transport } = await rig(today(() => [thread(T1)]));
  expect(talks.view().canRetry).toBe(false);
  await expect(talks.retry(T1, QUESTION, 1)).rejects.toBeDefined();
  expect(transport.only("next_command")).toEqual([]);
});

test("a refused retry rejects and opens nothing", async () => {
  const { talks, settle } = await rig(
    retrying([thread(T1), thread(T2)], () => {
      throw new Error("the host refused");
    }),
  );
  const before = talks.view();
  await expect(talks.retry(T2, QUESTION, 1)).rejects.toBeDefined();
  await settle();
  const after = talks.view();
  expect(after.open).toBe(before.open);
  expect(after.thread).toBe(T2);
  expect(threadsOf(after.rows)).toEqual([T2, T1]);
  expect(after.retried).toBeNull();
});

test("S6: a move made while retry_from is on its way stands; the new thread is listed, not opened", async () => {
  let answer: () => void = () => {};
  const held = new Promise<void>((resolve) => (answer = resolve));
  const { talks, settle } = await rig(
    retrying([thread(T1), thread(T2)], async () => {
      await held;
      return { thread_created: { thread: RETRIED } };
    }),
  );
  const retried = talks.retry(T2, QUESTION, 5);
  talks.open(T1);
  answer();
  await retried;
  await settle();
  const view = talks.view();
  expect(view.thread).toBe(T1);
  expect(view.open).toBe(T1);
  expect(view.retried).toBeNull();
  expect(threadsOf(view.rows)).toContain(RETRIED);
});
