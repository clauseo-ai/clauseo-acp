// A harness for Versions' tests: the real room (room.tsx) over a FakeTransport scripted for one matter, its thread's
// stream and the matter's own stream, with frames run by hand. Not shipped: only tests import it. Written with
// createElement, so this file holds no component.
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { StoreContext } from "../../store/hooks.ts";
import { createViewStore, type ViewStore } from "../../store/store.ts";
import { manualBatch } from "../../testing/dom.ts";
import {
  batch,
  event,
  header,
  MATTER,
  reset,
  STREAM,
  THREAD,
  TURN,
  uuid,
} from "../../testing/stream-script.ts";
import { FakeTransport } from "../../transport/fake-transport.ts";
import type {
  CheckpointKind,
  CommandOutcome,
  FileChange,
  HostCommand,
  HostQuery,
  MatterSummary,
  QueryResult,
  StepEntry,
  ThreadSummary,
  VersionEntry,
  ViewEntry,
  ViewPatchBody,
} from "../../transport/protocol.ts";
import { matterStreamOf } from "../matter-stream.ts";
import { Room } from "../room.tsx";
import { connectRoom } from "../start.ts";
import { loadThread } from "../thread/chunk.ts";
import { stubFonts } from "../thread/testing.ts";
import type { StandIns } from "./stand-ins.ts";
import { loadVersions } from "./use-versions.ts";

export const MATTER_STREAM = matterStreamOf(MATTER);
export const THE_MATTER: MatterSummary = {
  matter: MATTER,
  name: "Sharma v. State of Delhi",
  kind: "folder",
};
/** A second conversation of the matter. */
export const OTHER = uuid(2, 1);

/** Noon on 2 November 2026, in UTC: the versions' day in the tests, which run in UTC (vitest's TZ is the machine's;
 * each test reads times through the room's own clock words, so the zone doesn't change what it asserts). */
export const NOON = Date.UTC(2026, 10, 2, 12, 0);
const MINUTE = 60_000;

export const thread = (
  id = THREAD,
  title = "Draft reply",
  status: ThreadSummary["status"] = "idle",
): ThreadSummary => ({
  thread: id,
  matter: MATTER,
  agent: "claude",
  status,
  title,
});

/** A file a turn or the lawyer changed. */
export const file = (path: string, over: Partial<FileChange> = {}): FileChange => ({
  path,
  change: "modified",
  size_bytes: 100,
  may_include_lawyer_edits: false,
  ...over,
});

/** A version entry: the n-th checkpoint, `n` minutes past noon. */
export const version = (
  n: number,
  kind: CheckpointKind,
  over: Partial<VersionEntry> = {},
): ViewEntry => ({
  version: {
    order: n,
    checkpoint: `cp${n}`,
    kind,
    ts: NOON + n * MINUTE,
    files: [],
    ...over,
  },
});

/** A step of the thread: its turn, files and after-turn checkpoint. */
export const stepEntry = (order: number, over: Partial<StepEntry> = {}): ViewEntry => ({
  step: {
    order,
    turn: TURN,
    stop: "end_turn",
    files: [],
    skipped: [],
    undone: false,
    ts: NOON + order * MINUTE,
    ...over,
  },
});

/** The matter's header, for the matter stream's Reset. */
export const matterReset = (entries: ViewEntry[], imported = 3): ViewPatchBody => ({
  reset: {
    view_window: {
      header: {
        matter_header: {
          order: 0,
          matter: MATTER,
          name: THE_MATTER.name,
          kind: "folder",
          import: { files: imported, bytes: 1000, cloud_only: 0 },
        },
      },
      entries,
      has_older: false,
    },
  },
});

export type Script = {
  threads?: ThreadSummary[];
  dispatch?: (command: HostCommand) => CommandOutcome | Promise<CommandOutcome>;
  query?: (query: HostQuery) => QueryResult | Promise<QueryResult> | undefined;
  standIns?: StandIns;
};

export type VersionsHarness = {
  transport: FakeTransport;
  store: ViewStore;
  host: HTMLElement;
  /** Delivers one batch on a stream at its next cursors and runs a frame; the first on each must hold its Reset. */
  deliver: (stream: string, bodies: ViewPatchBody[]) => Promise<void>;
  settle: () => Promise<void>;
  one: (testId: string) => HTMLElement | null;
  all: (testId: string) => HTMLElement[];
  /** The commands dispatched, by kind. */
  sent: <K extends keyof HostCommand>(kind: K) => NonNullable<HostCommand[K]>[];
  click: (el: Element | null | undefined) => Promise<void>;
  /** Presses the toolbar's Versions, and waits for Versions' chunk. */
  openVersions: () => Promise<void>;
  /** The button in the dialog (or the page) whose text is `text`. */
  button: (text: string, within?: Element | null) => HTMLButtonElement | null;
  unmount: () => Promise<void>;
};

stubFonts();
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const flushAll = async () => {
  for (let i = 0; i < 4; i += 1) await new Promise((resolve) => setTimeout(resolve, 0));
};

/** The room with the thread's stream reset to `steps` (with its header) and the matter's stream to `versions`. */
export async function mountVersions(
  versions: ViewEntry[],
  steps: ViewEntry[] = [],
  script: Script = {},
): Promise<VersionsHarness> {
  const transport = new FakeTransport({
    query: async (query) => {
      if (query === "list_matters") return { matters: [THE_MATTER] };
      const scripted = await script.query?.(query);
      if (scripted !== undefined) return scripted;
      if (typeof query === "object" && query.list_threads !== undefined)
        return { threads: script.threads ?? [thread()] };
      throw new Error(`not scripted: ${JSON.stringify(query)}`);
    },
    dispatch: (envelope) => script.dispatch?.(envelope.command) ?? "done",
  });
  if (script.standIns !== undefined) Object.assign(transport, script.standIns);
  connectRoom(transport, {});
  const manual = manualBatch();
  const store = createViewStore(transport, { batch: () => manual.batch });
  const host = document.createElement("div");
  document.body.append(host);
  const root: Root = createRoot(host);
  await act(async () =>
    root.render(createElement(StoreContext, { value: store }, createElement(Room, { transport }))),
  );
  const settle = () => act(flushAll);
  await settle();
  await act(async () => void (await loadThread()));
  const cursors = new Map<string, number>();
  const deliver = async (stream: string, bodies: ViewPatchBody[]) => {
    const at = cursors.get(stream) ?? 1;
    transport.deliver(JSON.stringify(event(batch(at, bodies, undefined, stream))));
    cursors.set(stream, at + bodies.length);
    await act(async () => manual.run());
    await settle();
  };
  await deliver(STREAM, [reset(steps, header())]);
  const button = (text: string, within: Element | null = document.body) =>
    [...(within ?? document.body).querySelectorAll("button")].find(
      (b) => b.textContent?.trim() === text,
    ) ?? null;
  const click = async (el: Element | null | undefined) => {
    if (el == null) throw new Error("nothing to click");
    await act(async () => (el as HTMLElement).click());
    await settle();
  };
  return {
    transport,
    store,
    host,
    deliver,
    settle,
    one: (id) => document.body.querySelector<HTMLElement>(`[data-testid="${id}"]`),
    all: (id) => [...document.body.querySelectorAll<HTMLElement>(`[data-testid="${id}"]`)],
    sent: (kind) =>
      transport
        .only("dispatch")
        .map((c) => c.command.command[kind])
        .filter((c) => c !== undefined) as never,
    click,
    openVersions: async () => {
      await click(button("Versions", host.querySelector(".room-pair")));
      await act(async () => void (await loadVersions()));
      await settle();
      // the matter's stream, once Versions holds it
      if (!cursors.has(MATTER_STREAM)) await deliver(MATTER_STREAM, [matterReset(versions)]);
    },
    button,
    unmount: async () => {
      await act(async () => root.unmount());
      host.remove();
      store.dispose();
    },
  };
}
