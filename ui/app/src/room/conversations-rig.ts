// Shared setup for the conversations tests: a started Matters and Conversations over a FakeTransport, with the fixtures.
// Not shipped: only tests import it, and its afterEach runs in each file that imports it.
import { afterEach, vi } from "vitest";
import { settled } from "@clauseo/components/lib/late.ts";
import type { Batch } from "../store/batch.ts";
import { createViewStore } from "../store/store.ts";
import { batch, event, reset, uuid } from "../testing/stream-script.ts";
import { FakeTransport, type FakeHost } from "../transport/fake-transport.ts";
import type {
  AgentId,
  CommandOutcome,
  HostCommand,
  MatterId,
  MatterSummary,
  ThreadId,
  ThreadStatus,
  ThreadSummary,
  ViewEntry,
} from "../transport/protocol.ts";
import { Conversations, type ConversationRow } from "./conversations.ts";
import { detailed, fakeNext } from "./fake-next.ts";
import type { ThreadInfo } from "./host-next.ts";
import { Matters } from "./matters.ts";

export const A: MatterSummary = { matter: uuid(1, 2), name: "Acme Ltd v. State", kind: "folder" };
export const B: MatterSummary = {
  matter: uuid(2, 2),
  name: "Sharma v. State of Delhi",
  kind: "folder",
};
export const T1 = uuid(1, 1);
export const T2 = uuid(2, 1);
export const T3 = uuid(3, 1);
export const MADE = uuid(9, 1);

export const thread = (
  id: ThreadId,
  over: Partial<ThreadSummary> = {},
  matter: MatterId = A.matter,
): ThreadSummary => ({ thread: id, matter, agent: "claude-acp", status: "idle", ...over });

export const info = (
  id: ThreadId,
  over: Partial<ThreadInfo> = {},
  matter: MatterId = A.matter,
): ThreadInfo => ({ ...thread(id, {}, matter), seen: true, ...over });

export const stops: (() => void)[] = [];
afterEach(() => {
  for (const stop of stops.splice(0)) stop();
  vi.restoreAllMocks();
});

/** A started Matters and Conversations over one fake host, with a store whose frames run by hand. */
export async function rig(host: FakeHost, focused: () => boolean = () => false) {
  const transport = new FakeTransport(host);
  const pending: (() => void)[] = [];
  const manual: Batch = (flush) => {
    pending.push(flush);
    return () => {};
  };
  const store = createViewStore(transport, { batch: () => manual });
  const matters = new Matters(transport, store, manual);
  stops.push(matters.start());
  const drafts = new Map<string, string>();
  const talks = new Conversations(transport, store, matters, drafts, focused);
  stops.push(talks.start());
  const settle = async () => {
    // the answers in, then the late imports they started (sent()'s conversation-title), however deep the chain
    await new Promise((resolve) => setTimeout(resolve, 0));
    await settled();
    await new Promise((resolve) => setTimeout(resolve, 0));
    pending.splice(0).forEach((f) => f());
    await new Promise((resolve) => setTimeout(resolve, 0));
  };
  await settle();
  /** The commands dispatched, by kind. */
  const sent = <K extends keyof HostCommand>(kind: K) =>
    transport
      .only("dispatch")
      .map((c) => c.command.command[kind])
      .filter((c) => c !== undefined) as NonNullable<HostCommand[K]>[];
  const listReads = () =>
    transport.only("query").filter((c) => typeof c.query === "object" && "list_threads" in c.query)
      .length +
    transport
      .only("next_query")
      .filter((c) => typeof c.query === "object" && "list_threads" in c.query).length;
  const marks = () =>
    transport
      .only("next_command")
      .map((c) => c.command)
      .filter((c) => "mark_seen" in c);
  return { transport, store, matters, talks, drafts, settle, sent, listReads, marks };
}

/** Today's host: list_matters, list_threads per matter, create_thread making MADE, every other command done. */
export function today(
  threads: (matter: MatterId) => ThreadSummary[] | Promise<ThreadSummary[]>,
  matters: MatterSummary[] = [A],
  dispatch: (command: HostCommand) => CommandOutcome = (command) =>
    command.create_thread !== undefined ? { thread_created: { thread: MADE } } : "done",
): FakeHost {
  return {
    query: async (query) => {
      if (query === "list_matters") return { matters };
      if (typeof query === "object" && query.list_threads !== undefined)
        return { threads: await threads(query.list_threads.matter) };
      throw new Error(`not scripted: ${JSON.stringify(query)}`);
    },
    dispatch: (envelope) => dispatch(envelope.command),
  };
}

/** A NextHost with the matters, app and agents parts: A, opened last, with these threads. */
export function next(
  threads: (matter: MatterId) => ThreadInfo[],
  matters = [detailed(A)],
): FakeHost {
  return {
    next: fakeNext({ matters: () => matters, threads }),
    dispatch: (envelope) =>
      envelope.command.create_thread !== undefined ? { thread_created: { thread: MADE } } : "done",
  };
}

const headerOf = (
  id: ThreadId,
  status: ThreadStatus,
  over: { title?: string; agent?: AgentId } = {},
): ViewEntry => ({
  header: {
    order: 1,
    thread: id,
    matter: A.matter,
    agent: over.agent ?? "claude-acp",
    status,
    ...(over.title === undefined ? {} : { title: over.title }),
    config_options: [],
    commands: [],
    plan: [],
  },
});

/** Plays a reset of a thread's view holding only this header, at cursor `at`. */
export function play(
  transport: FakeTransport,
  id: ThreadId,
  status: ThreadStatus,
  at: number,
  over: { title?: string; agent?: AgentId } = {},
) {
  transport.deliver(
    event(batch(at, [reset([], headerOf(id, status, over))], uuid(1, 3), `thread:${id}`)),
  );
}

export const threadsOf = (rows: readonly ConversationRow[]) => rows.map((r) => r.thread);
