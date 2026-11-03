// @vitest-environment happy-dom
// U3's done-when 1: React commits while the room's thread takes a scripted 500-chunk, 20-tool-call stream, under a test
// scheduler (the pattern R12's internal window's test used, on the room). Frames run by hand (the store's `Batch`), so
// every number here is the same on every run. Then the thread at rest: no timer, no animation frame, no commit.
import {
  act,
  createElement,
  memo,
  Profiler,
  StrictMode,
  type ProfilerOnRenderCallback,
} from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeAll, expect, test, vi } from "vitest";
import { StoreContext } from "../../store/hooks.ts";
import { createViewStore } from "../../store/store.ts";
import { entryIdOf } from "../../store/view.ts";
import { manualBatch, stubLayout } from "../../testing/dom.ts";
import {
  agentText,
  append,
  batch,
  event,
  header,
  reset,
  scriptedTurn,
  toolCall,
  upsert,
  userMessage,
  uuid,
} from "../../testing/stream-script.ts";
import { FakeTransport } from "../../transport/fake-transport.ts";
import type { EntryId, StreamId, ViewEntry, ViewPatchBody } from "../../transport/protocol.ts";
import { Room } from "../room.tsx";
import { connectRoom } from "../start.ts";
import { defaultOutcome, summary, THE_MATTER } from "./harness.ts";
import { stubFonts } from "./testing.ts";
import { loadThread } from "./chunk.ts";
import { sameIds } from "./ids.ts";

// Each row gets its own Profiler, inside a memo boundary that matches the real row's, so a row's onRender fires only
// when that row's subtree renders.
const rowCommits = new Map<string, number>();
const countRow: ProfilerOnRenderCallback = (id) =>
  rowCommits.set(id, (rowCommits.get(id) ?? 0) + 1);
vi.mock("./rows.tsx", async (load) => {
  const real = await load<typeof import("./rows.tsx")>();
  return {
    ...real,
    Row: memo(function ProfiledRow(props: { id: EntryId; stream: StreamId }) {
      return createElement(
        Profiler,
        { id: props.id, onRender: countRow },
        createElement(real.Row, props),
      );
    }),
  };
});

// and each step group its own, by its ids and whether it is the running one's place (`tail`): a group counts as
// changed when a frame touches one of its rows, and as settled when it stops being the running group
const GROUP = "group:";
vi.mock("./step-group-live.tsx", async (load) => {
  const real = await load<typeof import("./step-group-live.tsx")>();
  return {
    ...real,
    LiveStepGroup: memo(function ProfiledGroup(props: GroupProps) {
      return createElement(
        Profiler,
        { id: `${GROUP}${props.tail} ${props.ids.join(" ")}`, onRender: countRow },
        createElement(real.LiveStepGroup, props),
      );
    }, sameGroup),
  };
});

type GroupProps = {
  ids: readonly EntryId[];
  stream: StreamId;
  paged: ReadonlyMap<EntryId, ViewEntry>;
  tail: boolean;
};

/** The real group's own memo test: the same ids, stream, older pages and place. */
const sameGroup = (a: GroupProps, b: GroupProps) =>
  a.stream === b.stream && a.tail === b.tail && a.paged === b.paged && sameIds(a.ids, b.ids);

/** A group's commit: changed when the frame touched one of its rows, settled when its `tail` went from what the
 * group of the same first row had, or null for a row's. */
function groupCommit(
  id: string,
  touched: ReadonlySet<string>,
  tails: Map<string, string>,
): "changed" | "settled" | "unchanged" | null {
  if (!id.startsWith(GROUP)) return null;
  const [tail, ...ids] = id.slice(GROUP.length).split(" ");
  const was = tails.get(ids[0]!);
  tails.set(ids[0]!, tail!);
  if (ids.some((i) => touched.has(i))) return "changed";
  return was !== undefined && was !== tail ? "settled" : "unchanged";
}

beforeAll(() => {
  stubLayout();
  stubFonts();
});

let root: Root | null = null;
afterEach(() => {
  act(() => root?.unmount());
  root = null;
  rowCommits.clear();
  vi.useRealTimers();
});

/** Patches the bridge delivers in one frame: the host's 16 ms display batch holds about three agent chunks. */
const PER_FRAME = 3;

/** The entry ids a frame's patches change. */
function touchedBy(bodies: readonly ViewPatchBody[]): Set<string> {
  const touched = new Set<string>();
  for (const body of bodies) {
    if (body.upsert !== undefined) touched.add(entryIdOf(body.upsert.entry));
    if (body.append_text !== undefined) touched.add(`agent_text:${body.append_text.message_id}`);
  }
  return touched;
}

test("500 chunks and 20 tool calls into the room's thread: one commit a frame, none for an unchanged row; at rest, no timer, frame or commit", async () => {
  // timers and frames are the test's: the rest check counts them
  vi.useFakeTimers({
    toFake: [
      "setTimeout",
      "clearTimeout",
      "setInterval",
      "clearInterval",
      "requestAnimationFrame",
      "cancelAnimationFrame",
    ],
  });
  const settle = () => act(() => vi.advanceTimersByTimeAsync(0));
  const transport = new FakeTransport({
    query: (query) =>
      query === "list_matters" ? { matters: [THE_MATTER] } : { threads: [summary()] },
    dispatch: (envelope) => defaultOutcome(envelope.command),
  });
  connectRoom(transport, {});
  const manual = manualBatch();
  const frame = () => act(manual.run);
  const store = createViewStore(transport, { batch: () => manual.batch });
  const phases: string[] = [];
  const onRender: ProfilerOnRenderCallback = (_id, phase) => phases.push(phase);
  const container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
  // awaited: Work's transcript suspends on the thread's chunk as it mounts, and only an act React can wait on lets the
  // room's effects run past it
  await act(async () =>
    root!.render(
      createElement(
        StrictMode,
        null,
        createElement(
          Profiler,
          { id: "room", onRender },
          createElement(StoreContext, { value: store }, createElement(Room, { transport })),
        ),
      ),
    ),
  );
  await settle();
  // Work started the thread's chunk as it mounted; the thread opens once it is in, before the first answer
  await act(async () => void (await loadThread()));
  await settle();
  const stream = transport.only("subscribe")[0]?.stream;
  expect(stream).toBe(`thread:${summary().thread}`);
  const prompt = userMessage(5, 1, "What does clause nine say?");
  transport.deliver(JSON.stringify(event(batch(1, [reset([prompt], header("running"))]))));
  frame();
  await settle();

  const { bodies, chunks, tools } = scriptedTurn();
  expect([chunks, tools]).toEqual([500, 20]);
  let frames = 0;
  let maxCommits = 0;
  let framesOverOneCommit = 0;
  let changedRowCommits = 0;
  let unchangedRowCommits = 0;
  let settledRowCommits = 0;
  let cursor = 2;
  const seen = new Set<string>([entryIdOf(prompt)]);
  const tails = new Map<string, string>();
  for (let i = 0; i < bodies.length; i += PER_FRAME) {
    const slice = bodies.slice(i, i + PER_FRAME);
    for (const body of slice) transport.deliver(JSON.stringify(event(batch(cursor++, [body]))));
    const touched = touchedBy(slice);
    for (const id of touched) seen.add(id);
    const streamingBefore = store.view(stream!)?.latestOutput ?? null;
    rowCommits.clear();
    phases.length = 0;
    frame();
    // an answer that stops being the latest output renders once more, settled (#123's final render: its code
    // coloured, its links live); its own part changed, so it counts apart from the unchanged rows. So does a step
    // group that stops being the running one as the next answer starts after it (groupCommit)
    const streamingAfter = store.view(stream!)?.latestOutput ?? null;
    const settled =
      streamingBefore !== streamingAfter && streamingBefore?.startsWith("agent_text:") === true
        ? streamingBefore
        : null;
    frames += 1;
    maxCommits = Math.max(maxCommits, phases.length);
    if (phases.length > 1) framesOverOneCommit += 1;
    for (const [id, n] of rowCommits) {
      const group = groupCommit(id, touched, tails);
      if (group === "changed") changedRowCommits += n;
      else if (group === "settled") settledRowCommits += n;
      else if (group === "unchanged") unchangedRowCommits += n;
      else if (touched.has(id)) changedRowCommits += n;
      else if (id === settled) settledRowCommits += n;
      else unchangedRowCommits += n;
    }
  }

  expect(container.querySelectorAll("[data-entry-id]")).toHaveLength(seen.size);
  expect(store.view(stream!)?.cursor).toBe(cursor - 1);
  // the brief's gates: `react_commits_per_frame` = 1 on every frame, new rows included, and
  // `react_commits_unchanged_rows` = 0
  expect(maxCommits).toBe(1);
  expect(framesOverOneCommit).toBe(0);
  expect(unchangedRowCommits).toBe(0);

  // the pen writes in the working line while the turn runs, on frames of its own: a second of them commits nothing
  const pen = container.querySelector<SVGElement>(".working-pen");
  expect(pen).not.toBeNull();
  const asked = vi.spyOn(window, "requestAnimationFrame");
  phases.length = 0;
  await act(() => vi.advanceTimersByTimeAsync(1000));
  expect(asked.mock.calls.length).toBeGreaterThan(30);
  expect(phases).toEqual([]);
  asked.mockRestore();

  // at rest: the turn ended; once the one-shot work of the end has run (the entity table's fetch), nothing waits on
  // a timer or a frame, and five seconds pass without a commit
  transport.deliver(
    JSON.stringify(event(batch(cursor++, [{ upsert: { entry: header("idle") } }]))),
  );
  frame();
  await act(async () => {
    await vi.runAllTimersAsync();
    await vi.dynamicImportSettled();
  });
  phases.length = 0;
  expect(vi.getTimerCount()).toBe(0);
  await act(() => vi.advanceTimersByTimeAsync(5000));
  expect(vi.getTimerCount()).toBe(0);
  expect(phases).toEqual([]);

  expect({
    frames,
    maxCommits,
    framesOverOneCommit,
    unchangedRowCommits,
    settledRowCommits,
    changedRowCommits,
  }).toMatchInlineSnapshot(`
    {
      "changedRowCommits": 222,
      "frames": 194,
      "framesOverOneCommit": 0,
      "maxCommits": 1,
      "settledRowCommits": 12,
      "unchangedRowCommits": 0,
    }
  `);
});

test("a pending permission while an answer streams: one commit a frame, and the permission row never renders on a flush", async () => {
  const transport = new FakeTransport({
    query: (query) =>
      query === "list_matters" ? { matters: [THE_MATTER] } : { threads: [summary()] },
    dispatch: (envelope) => defaultOutcome(envelope.command),
  });
  connectRoom(transport, {});
  const manual = manualBatch();
  const frame = () => act(manual.run);
  const store = createViewStore(transport, { batch: () => manual.batch });
  const phases: string[] = [];
  const onRender: ProfilerOnRenderCallback = (_id, phase) => phases.push(phase);
  const container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
  await act(async () =>
    root!.render(
      createElement(
        Profiler,
        { id: "room", onRender },
        createElement(StoreContext, { value: store }, createElement(Room, { transport })),
      ),
    ),
  );
  await act(async () => void (await loadThread()));
  const tool = toolCall(6, 300, "pending");
  const permission: ViewEntry = {
    permission: {
      order: 7,
      request: uuid(1, 8),
      tool_call: tool.tool_call!.tool_call,
      options: [
        { option_id: "once", kind: "allow_once", agent_label: "Allow" },
        { option_id: "no", kind: "reject_once", agent_label: "Reject" },
      ],
      deliverable: "live",
      state: "pending",
    },
  };
  let cursor = 1;
  const send = (bodies: ViewPatchBody[]) => {
    for (const body of bodies) transport.deliver(JSON.stringify(event(batch(cursor++, [body]))));
    frame();
  };
  send([
    reset(
      [userMessage(5, 1, "What does clause nine say?"), tool, permission, agentText(8, 100)],
      header("needs_you"),
    ),
  ]);
  const permissionId = entryIdOf(permission);
  expect(container.querySelector('[data-testid="permission"]')?.getAttribute("data-state")).toBe(
    "pending",
  );
  let maxCommits = 0;
  let len = 0;
  for (let i = 0; i < 30; i += 1) {
    rowCommits.clear();
    phases.length = 0;
    const text = `word ${i} `;
    // a text flush, and now and then a change to another row
    const bodies: ViewPatchBody[] = [append(100, len, text)];
    if (i % 10 === 9) bodies.push(upsert(toolCall(9 + i, 400 + i, "pending")));
    send(bodies);
    len += text.length;
    maxCommits = Math.max(maxCommits, phases.length);
    expect(rowCommits.get(permissionId), `frame ${i}`).toBeUndefined();
    const touched = touchedBy(bodies);
    const rowsOf = (id: string) => (id.startsWith(GROUP) ? id.split(" ").slice(1) : [id]);
    expect([...rowCommits.keys()].filter((id) => !rowsOf(id).some((r) => touched.has(r)))).toEqual(
      [],
    );
  }
  expect(maxCommits).toBe(1);
  // the tool call it is about changing renders it, as it should
  rowCommits.clear();
  send([upsert(toolCall(6, 300, "in_progress"))]);
  expect(rowCommits.get(permissionId)).toBe(1);
});
