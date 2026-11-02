// @vitest-environment happy-dom
// #253: the speed scoreboard's play (ui/e2e/app-windows/scoreboard.spec.ts) through the real Room, frame by frame, with
// React commits counted the way the perf recorder counts them (perf/stream.ts): every `onCommitFiberRoot` between one
// frame and the next. A frame here starts at the store's flush (the recorder's loop callback runs just before it in
// the same frame) and holds every commit after it until the next flush: IPC replies, clicks, React's own tasks.
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeAll, expect, test, vi } from "vitest";

// A DevTools hook installed before react-dom loads: counts commits as the page's perf hook does, and names the
// function components that rendered in each one.
const hook = vi.hoisted(() => {
  type Fiber = {
    type: unknown;
    child: Fiber | null;
    sibling: Fiber | null;
    alternate: Fiber | null;
    memoizedProps: unknown;
    memoizedState: unknown;
    actualStartTime?: number;
  };
  const state = { commits: [] as string[][], since: 0 };
  const nameOf = (t: unknown): string | null => {
    if (typeof t === "function")
      return (t as { displayName?: string; name?: string }).displayName ?? t.name ?? "anon";
    if (t !== null && typeof t === "object") {
      const o = t as { type?: unknown; render?: unknown };
      if (o.render !== undefined) return nameOf(o.render);
      if (o.type !== undefined) return nameOf(o.type);
    }
    return null;
  };
  const hookObj = {
    supportsFiber: true,
    isDisabled: false,
    renderers: new Map(),
    inject: () => 1,
    checkDCE: () => {},
    onScheduleFiberRoot: () => {},
    onCommitFiberUnmount: () => {},
    onPostCommitFiberRoot: () => {},
    onCommitFiberRoot: (_id: number, root: { current: Fiber }) => {
      const rendered = new Set<string>();
      const walk = (f: Fiber | null) => {
        for (let x = f; x !== null; x = x.sibling) {
          const name = nameOf(x.type);
          const fresh = (x.actualStartTime ?? -1) >= state.since;
          const changed =
            x.alternate === null ||
            x.alternate.memoizedProps !== x.memoizedProps ||
            x.alternate.memoizedState !== x.memoizedState;
          if (name !== null && fresh && changed) rendered.add(name);
          walk(x.child);
        }
      };
      walk(root.current);
      state.commits.push([...rendered]);
      state.since = performance.now();
    },
  };
  (globalThis as { __REACT_DEVTOOLS_GLOBAL_HOOK__?: unknown }).__REACT_DEVTOOLS_GLOBAL_HOOK__ =
    hookObj;
  return state;
});

import { StoreContext } from "../../store/hooks.ts";
import { createViewStore } from "../../store/store.ts";
import { manualBatch, stubLayout } from "../../testing/dom.ts";
import {
  agentText,
  append,
  batch,
  chosen,
  editCall,
  event,
  header,
  permission,
  reset,
  step,
  THREAD,
  TURN,
  upsert,
  userMessage,
} from "../../testing/stream-script.ts";
import { FakeTransport } from "../../transport/fake-transport.ts";
import type { CommandOutcome, HostCommand, ViewPatchBody } from "../../transport/protocol.ts";
import { Room } from "../room.tsx";
import { connectRoom } from "../start.ts";
import { loadThread } from "./chunk.ts";
import { summary, THE_MATTER } from "./harness.ts";
import { stubFonts } from "./testing.ts";

beforeAll(() => {
  stubLayout();
  stubFonts();
});

let root: Root | null = null;
afterEach(() => {
  act(() => root?.unmount());
  root = null;
});

type Deferred<T> = { promise: Promise<T>; resolve: (v: T) => void };
const deferred = <T,>(): Deferred<T> => {
  let resolve!: (v: T) => void;
  const promise = new Promise<T>((r) => (resolve = r));
  return { promise, resolve };
};
const tick = () => new Promise((r) => setTimeout(r, 0));
const settle = () =>
  act(async () => {
    for (let i = 0; i < 3; i += 1) await tick();
  });

/** When a click's IPC reply lands, against the host's patch for what the click did:
 * - fast: in the click's own frame (the reply's round trip under 16 ms); the patch is flushed in the next frame;
 * - patch-first: the patch is flushed in the next frame, and the reply lands after that flush, before the frame after;
 * - apart: the reply in the next frame's gap, the patch a frame later. */
type Order = "fast" | "patch-first" | "apart";

// Off the act environment: React schedules as in the page (sync lane in a microtask, the rest in Scheduler tasks), and
// a frame's window holds whatever commits before the next frame.
const tasks = async () => {
  for (let i = 0; i < 6; i += 1) await tick();
};

/** Where the play starts:
 * - "open": the matter has a resting thread, open and empty;
 * - "new": the matter was just made, as the play's is (openMatter creates it): the first Ask runs create_thread, which
 *   answers in the click's frame, then send_prompt; the new thread's first patch is its Reset, holding the question and
 *   the running header (the host takes send_prompt before the window's subscribe, so the Reset comes after it);
 * - "new, idle Reset first": as "new", but the new thread's Reset, resting and empty, is flushed before send_prompt's
 *   reply and the question comes as patches after. */
type Start = "open" | "new" | "new, idle Reset first";

async function play(sendOrder: Order, allowOrder: Order, start: Start) {
  const fresh = start !== "open";
  let sendReply: Deferred<CommandOutcome> | null = null;
  let createReply: Deferred<CommandOutcome> | null = null;
  let renameReply: Deferred<CommandOutcome> | null = null;
  const transport = new FakeTransport({
    query: (q) =>
      q === "list_matters" ? { matters: [THE_MATTER] } : { threads: fresh ? [] : [summary()] },
    dispatch: (envelope) => {
      const c: HostCommand = envelope.command;
      if (c.create_thread !== undefined) {
        createReply = deferred();
        return createReply.promise;
      }
      // U6: the first accepted question titles its thread (conversations.ts `sent`): its own round trip
      if (c.rename_thread !== undefined) {
        renameReply = deferred();
        return renameReply.promise;
      }
      if (c.send_prompt !== undefined) {
        sendReply = deferred();
        return sendReply.promise;
      }
      return "done";
    },
  });
  let answerReply: Deferred<void> | null = null;
  transport.answerPermission = async (thread, request, option) => {
    transport.calls.push({ call: "answer_permission", thread, request, option });
    answerReply = deferred();
    return answerReply.promise;
  };
  connectRoom(transport, {});
  const manual = manualBatch();
  const store = createViewStore(transport, { batch: () => manual.batch });
  const host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () =>
    root!.render(createElement(StoreContext, { value: store }, createElement(Room, { transport }))),
  );
  await settle();
  await act(async () => void (await loadThread()));
  await settle();
  let cursor = 1;
  const arrive = (bodies: ViewPatchBody[]) => {
    transport.deliver(JSON.stringify(event(batch(cursor, bodies))));
    cursor += bodies.length;
  };
  const one = (id: string) => host.querySelector<HTMLElement>(`[data-testid="${id}"]`);
  // the thread as the matter opens: resting, empty
  if (!fresh) {
    arrive([reset([], header("idle"))]);
    await act(async () => manual.run());
    await settle();
  }
  // the prompt typed (before the recording, as the play does)
  const field = one("composer") as HTMLTextAreaElement;
  await act(async () => {
    Reflect.set(HTMLTextAreaElement.prototype, "value", "Review the MSA.", field);
    field.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await settle();
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = false;

  // recording: frames[i] = what committed from frame i's flush up to frame i+1's
  const frames: { label: string; commits: string[][] }[] = [];
  let label = "recording starts";
  let mark = hook.commits.length;
  const close = () => {
    frames.push({ label, commits: hook.commits.slice(mark) });
    mark = hook.commits.length;
  };
  /** One frame: its flush (if anything waits), then the tasks of the gap before the next. */
  const frame = async (next: string) => {
    close();
    label = next;
    manual.run();
    await tasks();
  };
  const idle = async (n = 2) => {
    for (let i = 0; i < n; i += 1) await frame("idle");
  };
  const click = async (el: HTMLElement | null) => {
    if (el === null) throw new Error("nothing to click");
    el.click();
    await tasks();
  };
  /** A click, its reply and the host's patch for it, in `order`. */
  const answered = async (
    order: Order,
    what: string,
    el: HTMLElement | null,
    reply: () => void,
    patch: ViewPatchBody[],
    between?: () => Promise<void>,
  ) => {
    await frame(`${what}: click`);
    await click(el);
    await between?.();
    const replied = async () => {
      reply();
      await tasks();
    };
    if (order === "fast") {
      await replied();
      arrive(patch);
      await frame(`${what}: patch`);
    } else if (order === "patch-first") {
      arrive(patch);
      await frame(`${what}: patch, then reply`);
      await replied();
    } else {
      await frame(`${what}: reply`);
      await replied();
      arrive(patch);
      await frame(`${what}: patch`);
    }
  };

  await idle();
  await answered(
    sendOrder,
    "send",
    one("send"),
    () => sendReply!.resolve({ prompt_accepted: { turn: TURN, message: "m" } }),
    start === "new"
      ? [reset([userMessage(5, 1, "Review the MSA.")], header("running"))]
      : [upsert(userMessage(5, 1, "Review the MSA.")), upsert(header("running"))],
    fresh
      ? async () => {
          // create_thread answers in the click's frame
          expect(createReply).not.toBeNull();
          createReply!.resolve({ thread_created: { thread: THREAD } });
          await tasks();
          if (start === "new, idle Reset first") {
            arrive([reset([], header("idle"))]);
            await frame("send: create_thread's idle Reset");
          }
        }
      : undefined,
  );
  // thoughts
  arrive([upsert(agentText(6, 10, "", "thought"))]);
  await frame("thought row");
  for (let i = 0, len = 0; i < 4; i += 1) {
    const t = `thinking ${i} `;
    arrive([append(10, len, t)]);
    len += t.length;
    await frame(`thought chunk ${i}`);
    // rename_thread's reply comes back while the thoughts stream
    const rename = renameReply as Deferred<CommandOutcome> | null;
    if (i === 1 && rename !== null) {
      rename.resolve("done");
      await tasks();
    }
  }
  // the tool call and its permission
  arrive([upsert(editCall(7, 20, "pending"))]);
  await frame("tool call");
  arrive([upsert(permission(8, 30, "pending", 20)), upsert(header("needs_you"))]);
  await frame("permission pending");
  await idle();
  await answered(
    allowOrder,
    "allow once",
    one("permission-allow_once"),
    () => answerReply!.resolve(),
    [
      upsert(permission(8, 30, chosen("allow_once"), 20)),
      upsert(editCall(7, 20, "in_progress")),
      upsert(header("running")),
    ],
  );
  arrive([upsert(editCall(7, 20, "completed"))]);
  await frame("tool completed");
  // the answer
  arrive([upsert(agentText(9, 40))]);
  await frame("answer row");
  for (let i = 0, len = 0; i < 6; i += 1) {
    const t = `word ${i} `;
    arrive([append(40, len, t)]);
    len += t.length;
    await frame(`answer chunk ${i}`);
  }
  // the end
  arrive([upsert(step(11)), upsert(header("idle"))]);
  await frame("step + idle");
  await idle(3);
  close();
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const answer = host.querySelector('[data-testid="permission"]')?.getAttribute("data-state");
  return { frames, answer };
}

type Frames = { label: string; commits: string[][] }[];
const show = (frames: Frames) =>
  frames
    .filter((f) => f.commits.length > 0)
    .map(
      (f) =>
        `  [${f.commits.length}] ${f.label}` +
        (f.commits.length > 1
          ? "\n" +
            f.commits
              .map((c, i) => `      commit ${i + 1}: ${c.slice(0, 40).join(", ")}`)
              .join("\n")
          : ""),
    )
    .join("\n");

const over = (frames: Frames) =>
  frames.filter((f) => f.commits.length > 1).map((f) => [f.label, f.commits.length]);

// The play as CI runs it: a new conversation, so the first Ask makes the thread (conversations.ts `create`, whose
// create_thread answers within the click's frame), then send_prompt, whose reply comes a frame later, before the new
// thread's Reset; rename_thread (the first question titles the thread) answers while the thoughts stream; Allow once's
// reply a frame after the click, the patch folding the request a frame after that. Before U6's fixes two frames committed
// twice: create's (the conversations' emit, then session.ts's setMade at another lane) and rename's (its emit beside
// that frame's flush). With a thread already open, send_prompt's reply committed twice too: the no-op setRefused(null)
// Ask's click queued at the sync lane, skipped by setSentTurn's render (refuse() in session.ts keeps it from being made).
test("the scoreboard's play as CI runs it: one commit a frame", async () => {
  const { frames } = await play("apart", "apart", "new");
  const counts = frames.map((f) => f.commits.length).filter((n) => n > 0);
  // on a failure, each frame with its commits and what rendered in each
  expect(over(frames), show(frames)).toEqual([]);
  expect(Math.max(...counts)).toBe(1);
  expect({
    framesWithCommits: counts.length,
    commits: counts.reduce((a, b) => a + b, 0),
    maxCommits: Math.max(...counts),
    framesOverOneCommit: counts.filter((n) => n > 1).length,
  }).toMatchInlineSnapshot(`
    {
      "commits": 22,
      "framesOverOneCommit": 0,
      "framesWithCommits": 22,
      "maxCommits": 1,
    }
  `);
});

// What can still commit twice is a reply's commit meeting another in one frame by its timing alone, none of it on the
// play as CI runs it: with a new matter, create_thread's and send_prompt's replies in one frame; send_prompt's or
// answer_permission's reply landing just after the flush of its patch; Allow once's click and its reply in one frame.
// Taking each reply's state into the frame's flush (as conversations.ts's rename does with `store.inFrame`) would end
// those too; private issue #253's follow-up tracks it.
