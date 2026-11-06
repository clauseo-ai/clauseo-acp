// The first read (UD2, room/first-read/), over a FakeTransport with the setup wire: it starts once, only after the plan
// is signed in, only for a matter added in this first run with at least one paper, never where a "First read" thread
// exists, and never twice; its question shows as "Clauseo's first read" by its accepted turn; the line says how far it
// has got; Stop sends cancel and Restart one more send_prompt with the same text, no restart_agent.
// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, test } from "vitest";
import {
  agentText,
  batch,
  event,
  header,
  reset,
  step,
  THREAD,
  TURN,
  upsert,
  userMessage,
} from "../../testing/stream-script.ts";
import { FakeTransport, type FakeHost } from "../../transport/fake-transport.ts";
import type {
  CommandOutcome,
  MatterSummary,
  PaperSummary,
  SetupEvent,
  ThreadSummary,
  ViewEntry,
} from "../../transport/protocol.ts";
import { fakeNext } from "../fake-next.ts";
import { Room } from "../room.tsx";
import { stubFonts } from "../thread/testing.ts";
import { connectRoom } from "../start.ts";
import {
  A,
  byId,
  CLAUDE,
  info,
  install,
  MADE_MATTER,
  play,
  press,
  READY,
  sent,
  settle,
  signIn,
  SIGNED_IN,
} from "../first-run/rig.tsx";
import { forgetMarksForTests, MARKS_KEY } from "./marks.ts";
import { FIRST_READ_PROMPT } from "./prompt.ts";

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

type Options = {
  matters?: MatterSummary[];
  papers?: PaperSummary[];
  threads?: ThreadSummary[];
  entries?: ViewEntry[];
};

/** The room over a host with the setup wire, the papers part, and the thread's view as it subscribes. */
async function mount(o: Options = {}) {
  let matters = o.matters ?? [];
  // the thread the read made, listed once made, as threads_changed would list it
  let made: ThreadSummary[] = [];
  const fake: FakeHost = {
    platform: "mac",
    query: async (query) => {
      if (query === "list_matters") return { matters };
      if (query === "get_settings") throw new Error("not scripted");
      if (typeof query === "object" && "list_threads" in query)
        return { threads: [...(o.threads ?? []), ...made] };
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
      papers: o.papers ?? [PAPER],
      agents: [{ id: CLAUDE, kind: "claude" }],
      parts: ["agents", "papers"],
    }),
    subscribe: (stream) =>
      queueMicrotask(() => {
        const head = header("running");
        head.header!.title = "First read";
        transport.deliver(event(batch(1, [reset(o.entries ?? [], head)], undefined, stream)));
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

/** The first screen's Open the folder: the matter made from it is added during the first run. */
async function openFolder(transport: FakeTransport) {
  await press(byId("open-folder"));
  await settle();
  return transport;
}

const order = (t: FakeTransport) =>
  t
    .only("dispatch")
    .map((d) => Object.keys(d.command.command)[0])
    .filter((k) => ["create_thread", "rename_thread", "send_prompt", "restart_agent"].includes(k!));

test("it starts once the plan is signed in, for the matter the first run added: create_thread, rename_thread, send_prompt, once", async () => {
  const t = await openFolder(await mount());
  expect(order(t)).toEqual([]);
  await play(t, signIn(CLAUDE, SIGNED_IN));
  await settle();
  expect(order(t)).toEqual(["create_thread", "rename_thread", "send_prompt"]);
  expect(sent(t, "create_thread")).toEqual([{ matter: MADE_MATTER, agent: CLAUDE }]);
  expect(sent(t, "rename_thread")).toEqual([{ thread: THREAD, title: "First read" }]);
  expect(sent(t, "send_prompt")).toEqual([
    { thread: THREAD, content: [{ text: { text: FIRST_READ_PROMPT } }] },
  ]);
  // a second signed_in starts nothing more
  await play(t, SIGNED_OUT, signIn(CLAUDE, SIGNED_IN));
  await settle();
  expect(order(t)).toEqual(["create_thread", "rename_thread", "send_prompt"]);
  // and the mark survives a reload
  expect(JSON.parse(localStorage.getItem(MARKS_KEY) ?? "{}")[MADE_MATTER]).toMatchObject({
    thread: THREAD,
    turns: [TURN],
    papers: 1,
  });
});

test("no paper, a matter not added in this first run, or a First read thread already there: nothing starts", async () => {
  let t = await openFolder(await mount({ papers: [] }));
  await play(t, signIn(CLAUDE, SIGNED_IN));
  expect(order(t)).toEqual([]);
  await act(async () => root?.unmount());
  host?.remove();
  localStorage.removeItem(MARKS_KEY);
  forgetMarksForTests();
  t = await mount({ matters: [A] });
  await play(t, signIn(CLAUDE, SIGNED_IN));
  expect(order(t)).toEqual([]);
  await act(async () => root?.unmount());
  host?.remove();
  localStorage.removeItem(MARKS_KEY);
  forgetMarksForTests();
  t = await openFolder(
    await mount({
      threads: [
        { thread: THREAD, matter: MADE_MATTER, agent: CLAUDE, status: "idle", title: "First read" },
      ],
    }),
  );
  await play(t, signIn(CLAUDE, SIGNED_IN));
  expect(order(t)).toEqual([]);
});

const PROMPT = userMessage(2, 1, FIRST_READ_PROMPT);

test("its question shows as Clauseo's first read, never the prompt, and the line says how far the read has got", async () => {
  const reading: ViewEntry = {
    tool_call: {
      order: 3,
      tool_call: "00000000-0000-7000-8000-0000000000a7",
      turn: TURN,
      started_at: 3000,
      title: "Read",
      kind: "read",
      status: "in_progress",
      locations: [{ inside: { path: "papers/Board minutes.docx" } }],
      content: [],
    },
  };
  const t = await openFolder(await mount({ entries: [PROMPT, reading] }));
  await play(t, signIn(CLAUDE, SIGNED_IN));
  await settle();
  const question = byId("message-user");
  expect(question?.textContent).toContain("Clauseo's first read");
  expect(document.body.textContent).not.toContain(FIRST_READ_PROMPT);
  expect(byId("first-read")?.dataset["state"]).toBe("reading");
  expect(byId("first-read")?.textContent).toContain(
    "Reading 1 file. Now \u2068Board minutes.docx\u2069.",
  );
  await act(async () =>
    t.deliver(
      event(
        batch(
          2,
          [upsert(agentText(4, 1, "A plain summary.")), upsert(step(5))],
          undefined,
          `thread:${THREAD}`,
        ),
      ),
    ),
  );
  await settle();
  expect(byId("first-read")?.textContent).toContain("Read 1 file.");
});

test("Stop sends cancel; stopped, Restart sends one send_prompt with the same text and no restart_agent", async () => {
  const t = await openFolder(await mount({ entries: [PROMPT] }));
  await play(t, signIn(CLAUDE, SIGNED_IN));
  await settle();
  await press(byId("cancel"));
  expect(sent(t, "cancel")).toEqual([{ thread: THREAD }]);
  await act(async () =>
    t.deliver(
      event(batch(2, [upsert(step(5, { stop: "cancelled" }))], undefined, `thread:${THREAD}`)),
    ),
  );
  await settle();
  expect(byId("first-read")?.dataset["state"]).toBe("stopped");
  expect(byId("first-read")?.textContent).toContain("Clauseo stopped. The answer so far is kept.");
  await press(byId("first-read-restart"));
  await settle();
  expect(sent(t, "send_prompt")).toHaveLength(2);
  expect(sent(t, "send_prompt")[1]).toEqual({
    thread: THREAD,
    content: [{ text: { text: FIRST_READ_PROMPT } }],
  });
  expect(order(t)).not.toContain("restart_agent");
});
