// A harness for the room's thread tests: the real room (room.tsx) over a FakeTransport scripted for one matter, inside
// the page's view store with frames run by hand. Not shipped: only tests import it. Written with createElement, so
// this file holds no component.
import { act, createElement, Profiler, type ProfilerOnRenderCallback } from "react";
import { createRoot, type Root } from "react-dom/client";
import { StoreContext } from "../../store/hooks.ts";
import { createViewStore, type ViewStore } from "../../store/store.ts";
import { manualBatch } from "../../testing/dom.ts";
import { batch, EPOCH, event, MATTER, THREAD, TURN } from "../../testing/stream-script.ts";
import { FakeTransport, type FakeHost } from "../../transport/fake-transport.ts";
import type {
  CommandOutcome,
  HostCommand,
  HostQuery,
  MatterId,
  MatterSummary,
  QueryResult,
  StreamId,
  ThreadSummary,
  ViewPatchBody,
} from "../../transport/protocol.ts";
import type { UiSettings } from "../host-next.ts";
import { Room } from "../room.tsx";
import { connectRoom } from "../start.ts";
import { loadThread } from "./chunk.ts";
import { stubFonts } from "./testing.ts";

export const THE_MATTER: MatterSummary = {
  matter: MATTER,
  name: "Sharma v. State of Delhi",
  kind: "folder",
};

export const summary = (
  thread = THREAD,
  status: ThreadSummary["status"] = "idle",
): ThreadSummary => ({
  thread,
  matter: MATTER,
  agent: "claude-acp",
  status,
});

export type Script = {
  /** list_matters' answer; one matter by default. */
  matters?: MatterSummary[];
  /** list_threads' answer, or each matter's; the one scripted thread by default. */
  threads?: ThreadSummary[] | ((matter: MatterId) => ThreadSummary[]);
  /** Answers each command; by default create_thread makes THREAD, send_prompt is accepted for TURN, the rest done. */
  dispatch?: (command: HostCommand) => CommandOutcome | Promise<CommandOutcome>;
  /** Answers any other query. */
  query?: (query: HostQuery) => QueryResult;
  display?: Partial<UiSettings>;
  /** Whether mounting waits for the thread's chunk (chunk.ts: the list, its rows and the renderer); true by default. */
  waitForRenderer?: boolean;
  /** Hears every commit of the room. */
  onRender?: ProfilerOnRenderCallback;
  /** The host rows it fakes beyond today's host (room/host-next.ts), such as H10's restart. */
  next?: FakeHost["next"];
  /** The settings window's open (FakeHost's openSettings): with it the room has Settings. */
  openSettings?: FakeHost["openSettings"];
  /** UC's protection wire (FakeHost's protection): with it the room reads the computer's protection. */
  protection?: FakeHost["protection"];
};

export const defaultOutcome = (command: HostCommand): CommandOutcome => {
  if (command.create_thread !== undefined) return { thread_created: { thread: THREAD } };
  if (command.send_prompt !== undefined)
    return { prompt_accepted: { turn: TURN, message: "00000000-0000-7000-8000-00000000beef" } };
  return "done";
};

/** A script whose PreviewUndo answers as `preview` says (or throws its refusal); any other query is not scripted. The
 * answer is typed by hand where the query is (session.ts): the bindings don't carry PreviewUndo yet. */
export const previewUndoScript = (preview: () => unknown): Script => ({
  query: (query) => {
    if (typeof query === "object" && "preview_undo" in query) return preview() as never;
    throw new Error(`not scripted: ${JSON.stringify(query)}`);
  },
});

export type Harness = {
  transport: FakeTransport;
  store: ViewStore;
  host: HTMLElement;
  /** Delivers one batch at the next cursors and runs a frame. The first must hold the Reset. */
  deliver: (bodies: ViewPatchBody[]) => void;
  /** As `deliver`, inside an awaited act: for a render that may suspend (the thread's chunk still loading), whose
   * retry React schedules only for an act it can wait on. */
  deliverAwaited: (bodies: ViewPatchBody[]) => Promise<void>;
  /** As `deliver`, on another thread's stream, at that stream's own cursors. */
  deliverOn: (stream: StreamId, bodies: ViewPatchBody[]) => void;
  /** Lets the host's answers and React settle. */
  settle: () => Promise<void>;
  one: (testId: string) => HTMLElement | null;
  all: (testId: string) => HTMLElement[];
  /** The commands dispatched, by kind. */
  sent: <K extends keyof HostCommand>(kind: K) => NonNullable<HostCommand[K]>[];
  /** Types into the composer's field. */
  type: (text: string) => Promise<void>;
  /** A key in the composer's field; returns whether its default was prevented. */
  key: (init: KeyboardEventInit) => Promise<boolean>;
  click: (el: Element | null | undefined) => Promise<void>;
  unmount: () => Promise<void>;
};

stubFonts();
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const flushAll = async () => {
  for (let i = 0; i < 3; i += 1) await new Promise((resolve) => setTimeout(resolve, 0));
};

export async function mountRoom(script: Script = {}): Promise<Harness> {
  const transport = new FakeTransport({
    query: (query) => {
      if (query === "list_matters") return { matters: script.matters ?? [THE_MATTER] };
      if (typeof query === "object" && query.list_threads !== undefined) {
        const t = script.threads;
        return {
          threads: typeof t === "function" ? t(query.list_threads.matter) : (t ?? [summary()]),
        };
      }
      if (script.query !== undefined) return script.query(query);
      throw new Error(`not scripted: ${JSON.stringify(query)}`);
    },
    dispatch: (envelope) => (script.dispatch ?? defaultOutcome)(envelope.command),
    ...(script.next === undefined ? {} : { next: script.next }),
    ...(script.openSettings === undefined ? {} : { openSettings: script.openSettings }),
    ...(script.protection === undefined ? {} : { protection: script.protection }),
  });
  connectRoom(transport, script.display ?? {});
  const manual = manualBatch();
  const store = createViewStore(transport, { batch: () => manual.batch });
  const host = document.createElement("div");
  document.body.append(host);
  const root: Root = createRoot(host);
  const room = createElement(StoreContext, { value: store }, createElement(Room, { transport }));
  await act(async () =>
    root.render(
      script.onRender === undefined
        ? room
        : createElement(Profiler, { id: "room", onRender: script.onRender }, room),
    ),
  );
  const settle = () => act(flushAll);
  await settle();
  // Work started the thread's chunk as it mounted
  if (script.waitForRenderer !== false) await act(async () => void (await loadThread()));
  let cursor = 1;
  const cursors = new Map<StreamId, number>();
  const field = () => host.querySelector<HTMLTextAreaElement>('[data-testid="composer"]');
  return {
    transport,
    store,
    host,
    deliver: (bodies) => {
      transport.deliver(JSON.stringify(event(batch(cursor, bodies))));
      cursor += bodies.length;
      act(manual.run);
    },
    deliverAwaited: async (bodies) => {
      transport.deliver(JSON.stringify(event(batch(cursor, bodies))));
      cursor += bodies.length;
      await act(async () => manual.run());
    },
    deliverOn: (stream, bodies) => {
      const first = cursors.get(stream) ?? 1;
      transport.deliver(JSON.stringify(event(batch(first, bodies, EPOCH, stream))));
      cursors.set(stream, first + bodies.length);
      act(manual.run);
    },
    settle,
    one: (id) => host.querySelector<HTMLElement>(`[data-testid="${id}"]`),
    all: (id) => [...host.querySelectorAll<HTMLElement>(`[data-testid="${id}"]`)],
    sent: (kind) =>
      transport
        .only("dispatch")
        .map((c) => c.command.command[kind])
        .filter((c) => c !== undefined) as never,
    type: async (text) => {
      const el = field();
      if (el === null) throw new Error("no composer");
      await act(async () => {
        Reflect.set(HTMLTextAreaElement.prototype, "value", text, el);
        el.dispatchEvent(new Event("input", { bubbles: true }));
      });
    },
    key: async (init) => {
      const el = field();
      if (el === null) throw new Error("no composer");
      const e = new KeyboardEvent("keydown", { bubbles: true, cancelable: true, ...init });
      await act(async () => {
        el.dispatchEvent(e);
      });
      await settle();
      return e.defaultPrevented;
    },
    click: async (el) => {
      if (el == null) throw new Error("nothing to click");
      await act(async () => (el as HTMLElement).click());
      await settle();
    },
    unmount: async () => {
      await act(async () => root.unmount());
      host.remove();
      store.dispose();
    },
  };
}
