// conversations.ts when the host's words cross on the way: a list_threads answer travels apart from H4's
// threads_changed (a query's reply, not the app feed), so an answer the host read before a change can land after the
// change was heard. The change is the newer word and stands. The room-other e2e (ui/e2e/app-windows/room-other.spec.ts)
// met it: B, left while starting, reads the list again; B's turn queues behind A meanwhile; the answer that says
// starting lands last, and B's row said "Answering." instead of waiting for A.
import { expect, test } from "vitest";
import type { FakeHost } from "../transport/fake-transport.ts";
import type { ThreadStatus } from "../transport/protocol.ts";
import { rowWords } from "./conversation-status.ts";
import { A, info, MADE, play, rig, stops, T1, T2 } from "./conversations-rig.ts";
import { detailed, fakeNext } from "./fake-next.ts";
import type { ThreadInfo } from "./host-next.ts";

/** A NextHost whose list_threads reads the list when asked and answers once `answer` lets it, while `holding`. */
function heldLists(threads: () => ThreadInfo[], made: ThreadStatus | null = null) {
  const held: (() => void)[] = [];
  const control = { holding: false, created: null as (() => void) | null };
  const base = fakeNext({ matters: () => [detailed(A)], threads: () => threads() });
  const host: FakeHost = {
    next: {
      ...base,
      query: (query) => {
        const answer = base.query(query);
        if (!control.holding || typeof query !== "object" || !("list_threads" in query))
          return answer;
        return new Promise((resolve) => held.push(() => resolve(answer)));
      },
    },
    dispatch: (envelope) => {
      if (envelope.command.create_thread === undefined) return "done";
      if (made === null) return { thread_created: { thread: MADE } };
      return new Promise((resolve) => {
        control.created = () => resolve({ thread_created: { thread: MADE } });
      });
    },
  };
  return { host, control, answer: () => held.splice(0).forEach((go) => go()) };
}

const behind = (thread: string): ThreadStatus => ({ queued: { behind: thread } });

test("a list answer the host read before a threads_changed lands after it: the change stands", async () => {
  let list = [info(T1, { status: "running" }), info(T2, { status: "starting" })];
  const { host, control, answer } = heldLists(() => list);
  const { talks, store, transport, settle } = await rig(host);
  // B (T2) open, starting
  const release = store.watch(`thread:${T2}`);
  play(transport, T2, "starting", 1);
  await settle();
  // B is left while starting (A opens): its view goes, and the room reads the list again, which the host reads now
  control.holding = true;
  release();
  await settle();
  stops.push(store.watch(`thread:${T1}`));
  play(transport, T1, "running", 1);
  await settle();
  // then B's turn queues behind A, and the app feed says so before the list's answer is back
  list = [info(T1, { status: "running" }), info(T2, { status: behind(T1) })];
  transport.appEvent({
    threads_changed: {
      matter: A.matter,
      threads: [info(T2, { status: behind(T1) })],
      removed: [],
      all: false,
    },
  });
  await settle();
  expect(talks.view().rows.find((row) => row.thread === T2)?.status).toEqual(behind(T1));
  // the older answer lands last
  answer();
  await settle();
  const b = talks.view().rows.find((row) => row.thread === T2);
  expect(b?.status).toEqual(behind(T1));
  expect(b === undefined ? null : rowWords(b)).toEqual({ state: "waiting", behind: T1 });
});

test("a thread removed by a threads_changed stays gone when an older list answer lands", async () => {
  let list = [info(T1), info(T2)];
  const { host, control, answer } = heldLists(() => list);
  const { talks, store, transport, settle } = await rig(host);
  const release = store.watch(`thread:${T2}`);
  play(transport, T2, "running", 1);
  await settle();
  control.holding = true;
  release();
  await settle();
  stops.push(store.watch(`thread:${T1}`));
  play(transport, T1, "idle", 1);
  await settle();
  list = [info(T1)];
  transport.appEvent({
    threads_changed: { matter: A.matter, threads: [], removed: [T2], all: false },
  });
  await settle();
  answer();
  await settle();
  expect(talks.view().rows.map((row) => row.thread)).toEqual([T1]);
});

test("create keeps a status threads_changed gave the new thread before create_thread answered", async () => {
  const list = [info(T1, { status: "running" })];
  const { host, control } = heldLists(() => list, "starting");
  const { talks, transport, settle } = await rig(host);
  talks.startNew();
  const key = talks.view().open!;
  const made = talks.create(key);
  await settle();
  transport.appEvent({
    threads_changed: {
      matter: A.matter,
      threads: [info(MADE, { status: behind(T1) })],
      removed: [],
      all: false,
    },
  });
  await settle();
  control.created?.();
  expect(await made).toBe(MADE);
  await settle();
  expect(talks.view().rows.find((row) => row.thread === MADE)?.status).toEqual(behind(T1));
});
