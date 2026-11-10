// D-021: the conversation the lawyer just had stands under its matter on the rail as soon as it exists, and stays there
// through the host's later frames. The first read (UD2) opens its thread with its summary (first-run.tsx,
// conversations.openThread with `known`), as Find opens an archived one: a summary that isn't archived still takes its
// row. And a conversation started from the composer keeps its row through a whole list that predates it.
// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, test } from "vitest";
import { batch, event, header, reset, THREAD, TURN } from "../testing/stream-script.ts";
import { FakeTransport, type FakeHost } from "../transport/fake-transport.ts";
import type {
  CommandOutcome,
  MatterSummary,
  PaperSummary,
  SetupEvent,
  ThreadSummary,
} from "../transport/protocol.ts";
import { A, info as listed, MADE, next, rig, T1, T2, threadsOf } from "./conversations-rig.ts";
import { forgetMarksForTests, MARKS_KEY } from "./first-read/marks.ts";
import {
  byId,
  CLAUDE,
  info,
  install,
  MADE_MATTER,
  play,
  press,
  READY,
  settle,
  signIn,
  SIGNED_IN,
} from "./first-run/rig.tsx";
import { fakeNext } from "./fake-next.ts";
import { Room } from "./room.tsx";
import { stubFonts } from "./thread/testing.ts";
import { connectRoom } from "./start.ts";

stubFonts();

let host: HTMLDivElement | null = null;
let root: Root | null = null;

beforeEach(() => {
  localStorage.removeItem(MARKS_KEY);
  forgetMarksForTests();
});
afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  root = null;
  host = null;
});

const PAPER = { path: "Plaint.pdf", file: "file-1", size: 1000 } as unknown as PaperSummary;
const SIGNED_OUT: SetupEvent = signIn(CLAUDE, { signed_out: {} });

/** The room over a first run's host: a folder opened makes MADE_MATTER, and its first read's thread is listed once
 * made, as the host's list would list it. */
async function mount() {
  let matters: MatterSummary[] = [];
  let made: ThreadSummary[] = [];
  const fake: FakeHost = {
    platform: "mac",
    query: async (query) => {
      if (query === "list_matters") return { matters };
      if (query === "get_settings") throw new Error("not scripted");
      if (typeof query === "object" && "list_threads" in query) return { threads: [...made] };
      throw new Error(`not scripted: ${JSON.stringify(query)}`);
    },
    dispatch: (envelope): CommandOutcome => {
      const c = envelope.command;
      if (c.create_thread !== undefined) {
        made = [
          {
            thread: THREAD,
            matter: c.create_thread.matter,
            agent: CLAUDE,
            status: "running",
            title: "First read",
          },
        ];
        return { thread_created: { thread: THREAD } };
      }
      if (c.send_prompt !== undefined) return { prompt_accepted: { turn: TURN, message: THREAD } };
      if (c.create_matter !== undefined) {
        matters = [{ matter: MADE_MATTER, name: c.create_matter.name, kind: "folder" }];
        return { matter_created: { matter: MADE_MATTER } };
      }
      return "done";
    },
    pickFolder: () => ({ folder: "folder-ref-1", display: "Documents/Matters/Rathi Steels" }),
    setup: { first: () => [info(), install(CLAUDE, READY), SIGNED_OUT] },
    next: fakeNext({
      matters: () => [],
      papers: [PAPER],
      agents: [{ id: CLAUDE, kind: "claude" }],
      parts: ["agents", "papers"],
    }),
    subscribe: (stream) =>
      queueMicrotask(() => {
        const head = header("running");
        head.header!.title = "First read";
        transport.deliver(event(batch(1, [reset([], head)], undefined, stream)));
      }),
  };
  const transport: FakeTransport = new FakeTransport(fake);
  connectRoom(transport, { language: "en" });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root?.render(<Room transport={transport} />));
  await settle();
  return transport;
}

const rows = () => [...(host?.querySelectorAll<HTMLElement>('[data-testid="conversation"]') ?? [])];

test("the first read's conversation stands under its matter as soon as it is made, current", async () => {
  const t = await mount();
  await press(byId("open-folder"));
  await settle();
  await play(t, signIn(CLAUDE, SIGNED_IN));
  await settle();
  const titles = rows().map((r) => r.querySelector(".room-conv-title")?.textContent);
  expect(titles).toEqual(["First read"]);
  expect(rows()[0]?.getAttribute("aria-current")).toBe("true");
});

test("a thread opened with a summary not archived takes its row, through a whole list that predates it; Find's archived one doesn't", async () => {
  const { talks, transport, settle } = await rig(next(() => [listed(T1)]));
  // the first read's way: made elsewhere, opened with its summary before any list holds it
  talks.openThread(A.matter, MADE, {
    thread: MADE,
    matter: A.matter,
    agent: CLAUDE,
    status: "starting",
    title: "First read",
    queue_held: false,
  });
  expect(talks.view().thread).toBe(MADE);
  expect(threadsOf(talks.view().rows)).toEqual([MADE, T1]);
  // a whole list sent before it was made keeps it
  const whole = (threads: ReturnType<typeof listed>[]) =>
    transport.appEvent({ threads_changed: { matter: A.matter, threads, removed: [], all: true } });
  whole([listed(T1)]);
  await settle();
  expect(threadsOf(talks.view().rows)).toEqual([MADE, T1]);
  // the host lists it: still one row, and it stays when another conversation opens
  whole([listed(T1), listed(MADE, { title: "First read", status: "running" })]);
  await settle();
  talks.open(T1);
  expect(new Set(threadsOf(talks.view().rows))).toEqual(new Set([MADE, T1]));
  // Find's archived thread opens without a row
  talks.openThread(A.matter, T2, {
    thread: T2,
    matter: A.matter,
    agent: CLAUDE,
    status: "archived",
  });
  expect(talks.view().thread).toBe(T2);
  expect(threadsOf(talks.view().rows)).not.toContain(T2);
});
