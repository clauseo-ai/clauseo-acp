// @vitest-environment happy-dom
// Done-when 3 of R12: React commits for a scripted 500-chunk, 20-tool-call stream under a test scheduler. Frames run
// by hand (the store's `Batch`), so every number here is the same on every run.
import { act, memo, Profiler, StrictMode, type ProfilerOnRenderCallback } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeAll, expect, test, vi } from "vitest";
import { StoreContext } from "../store/hooks.ts";
import { createViewStore } from "../store/store.ts";
import { entryIdOf } from "../store/view.ts";
import { FakeTransport } from "../transport/fake-transport.ts";
import type { EntryId, StreamId, ViewPatchBody } from "../transport/protocol.ts";
import {
  batch,
  event,
  header,
  reset,
  scriptedTurn,
  STREAM,
  userMessage,
} from "../testing/stream-script.ts";
import { manualBatch, stubLayout } from "../testing/dom.ts";
import { ThreadContext } from "./session.ts";
import { Transcript } from "./transcript.tsx";

// Each row gets its own Profiler, inside a memo boundary that matches the real row's, so a row's onRender fires only
// when that row's subtree renders.
const rowCommits = new Map<string, number>();
const countRow: ProfilerOnRenderCallback = (id) =>
  rowCommits.set(id, (rowCommits.get(id) ?? 0) + 1);
vi.mock("./rows.tsx", async (load) => {
  const real = await load<typeof import("./rows.tsx")>();
  return {
    Row: memo(function ProfiledRow(props: { id: EntryId; stream: StreamId }) {
      return (
        <Profiler id={props.id} onRender={countRow}>
          <real.Row {...props} />
        </Profiler>
      );
    }),
  };
});

beforeAll(stubLayout);

let root: Root | null = null;
afterEach(() => {
  act(() => root?.unmount());
  root = null;
  rowCommits.clear();
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

test("a scripted 500-chunk, 20-tool-call stream commits once a frame and never for an unchanged row", () => {
  const transport = new FakeTransport();
  const manual = manualBatch();
  const frame = () => act(manual.run);
  const store = createViewStore(transport, { batch: () => manual.batch });
  const phases: string[] = [];
  const onRender: ProfilerOnRenderCallback = (_id, phase) => phases.push(phase);
  const container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
  const actions = { thread: "t", stream: STREAM, answer: () => {}, undo: () => {} };
  act(() =>
    root!.render(
      <StrictMode>
        <Profiler id="window" onRender={onRender}>
          <StoreContext value={store}>
            <ThreadContext value={actions}>
              <Transcript stream={STREAM} />
            </ThreadContext>
          </StoreContext>
        </Profiler>
      </StrictMode>,
    ),
  );
  const release = store.watch(STREAM);
  const prompt = userMessage(5, 1, "What does clause nine say?");
  transport.deliver(JSON.stringify(event(batch(1, [reset([prompt], header("running"))]))));
  frame();

  const { bodies, chunks, tools } = scriptedTurn();
  expect([chunks, tools]).toEqual([500, 20]);
  let frames = 0;
  let maxStoreCommits = 0;
  let maxCommitsNoNewRow = 0;
  let maxCommitsNewRow = 0;
  let framesWithNewRow = 0;
  let changedRowCommits = 0;
  let unchangedRowCommits = 0;
  let cursor = 2;
  const seen = new Set<string>([entryIdOf(prompt)]);
  for (let i = 0; i < bodies.length; i += PER_FRAME) {
    const slice = bodies.slice(i, i + PER_FRAME);
    for (const body of slice) transport.deliver(JSON.stringify(event(batch(cursor++, [body]))));
    const touched = touchedBy(slice);
    const added = [...touched].some((id) => !seen.has(id));
    for (const id of touched) seen.add(id);
    rowCommits.clear();
    phases.length = 0;
    frame();
    frames += 1;
    // "nested-update": a commit a layout effect scheduled inside this one, before the browser paints.
    const storeCommits = phases.filter((p) => p !== "nested-update").length;
    maxStoreCommits = Math.max(maxStoreCommits, storeCommits);
    if (added) {
      framesWithNewRow += 1;
      maxCommitsNewRow = Math.max(maxCommitsNewRow, phases.length);
    } else {
      maxCommitsNoNewRow = Math.max(maxCommitsNoNewRow, phases.length);
    }
    for (const [id, n] of rowCommits) {
      if (touched.has(id)) changedRowCommits += n;
      else unchangedRowCommits += n;
    }
  }
  release();

  expect(container.querySelectorAll("[data-entry-id]")).toHaveLength(seen.size);
  expect(store.view(STREAM)?.cursor).toBe(cursor - 1);
  expect(maxStoreCommits).toBe(1);
  expect(maxCommitsNoNewRow).toBe(1);
  expect(unchangedRowCommits).toBe(0);
  expect({
    frames,
    maxStoreCommits,
    maxCommitsNoNewRow,
    framesWithNewRow,
    maxCommitsNewRow,
    unchangedRowCommits,
    changedRowCommits,
  }).toMatchInlineSnapshot(`
    {
      "changedRowCommits": 222,
      "frames": 194,
      "framesWithNewRow": 41,
      "maxCommitsNewRow": 3,
      "maxCommitsNoNewRow": 1,
      "maxStoreCommits": 1,
      "unchangedRowCommits": 0,
    }
  `);
});

/** The scripted stream as the host's 16 ms display batch sends it: a frame's patches in one batch, consecutive appends
 * to one message merged into one `AppendText`. */
function coalesced(bodies: readonly ViewPatchBody[]): ViewPatchBody[][] {
  const frames: ViewPatchBody[][] = [];
  for (let i = 0; i < bodies.length; i += PER_FRAME) {
    const merged: ViewPatchBody[] = [];
    for (const body of bodies.slice(i, i + PER_FRAME)) {
      const last = merged.at(-1)?.append_text;
      const next = body.append_text;
      if (last !== undefined && next !== undefined && last.message_id === next.message_id) {
        merged[merged.length - 1] = { append_text: { ...last, text: last.text + next.text } };
      } else merged.push(body);
    }
    frames.push(merged);
  }
  return frames;
}

// The brief's IPC target, at most 1.3x the text bytes, is R13's to measure on the running app; a follow-up tracks it.
// This records where the encoding stands today: each ~11 B chunk rides in ~140 B of JSON envelope, so even one batch
// per frame is about 13x. Closing the gap is host-side (longer coalescing, a leaner encoding), not the window's.
test("IPC bytes against text bytes for the scripted stream", () => {
  const { bodies, textBytes } = scriptedTurn();
  const perChunk = new FakeTransport();
  bodies.forEach((body, i) => perChunk.deliver(JSON.stringify(event(batch(i + 2, [body])))));
  const perFrame = new FakeTransport();
  let cursor = 2;
  for (const frame of coalesced(bodies)) {
    perFrame.deliver(JSON.stringify(event(batch(cursor, frame))));
    cursor += frame.length;
  }
  expect({
    textBytes,
    ipcBytesOnePatchPerMessage: perChunk.bytes,
    ipcBytesOneBatchPerFrame: perFrame.bytes,
    ratioOnePatchPerMessage: Math.round((perChunk.bytes / textBytes) * 10) / 10,
    ratioOneBatchPerFrame: Math.round((perFrame.bytes / textBytes) * 10) / 10,
  }).toMatchInlineSnapshot(`
    {
      "ipcBytesOneBatchPerFrame": 74343,
      "ipcBytesOnePatchPerMessage": 167856,
      "ratioOneBatchPerFrame": 13.2,
      "ratioOnePatchPerMessage": 29.8,
      "textBytes": 5639,
    }
  `);
});
