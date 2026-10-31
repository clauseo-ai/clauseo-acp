// @vitest-environment happy-dom
// The internal window against a fake host: reopen, new matter, send and cancel, permission buttons, undo, limit stop,
// error copy. Everything runs under Strict Mode, as main.tsx renders it.
import { act, StrictMode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeAll, expect, test } from "vitest";
import { App } from "../App.tsx";
import { FakeTransport, type FakeHost } from "../transport/fake-transport.ts";
import type { ViewEntry, ViewPatchBody } from "../transport/protocol.ts";
import { manualBatch, stubLayout } from "../testing/dom.ts";
import {
  agentText,
  append,
  batch,
  event,
  header,
  MATTER,
  reset,
  step,
  STREAM,
  THREAD,
  TURN,
  userMessage,
  uuid,
} from "../testing/stream-script.ts";
import { ERROR_COPY, PERMISSION_LABEL } from "./words.ts";

beforeAll(stubLayout);

let root: Root | null = null;
let container: HTMLElement;
afterEach(() => {
  act(() => root?.unmount());
  root = null;
  container?.remove();
});

const REOPEN: FakeHost = {
  query: (q) =>
    q === "list_matters"
      ? { matters: [{ matter: MATTER, name: "Lease review", kind: "folder" }] }
      : { threads: [{ thread: THREAD, matter: MATTER, agent: "claude", status: "idle" }] },
};

async function open(host: FakeHost = REOPEN) {
  const transport = new FakeTransport(host);
  // Counts every hand-off to a store listener. Each subscribe wraps its listener anew, so a second subscription
  // from a double effect would show as two hand-offs per batch.
  let handed = 0;
  const subscribe = transport.subscribe.bind(transport);
  transport.subscribe = (stream, after, listener) =>
    subscribe(stream, after, (b) => {
      handed += 1;
      listener(b);
    });
  const frames = manualBatch();
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
  await act(async () => {
    root!.render(
      <StrictMode>
        <App transport={transport} batch={frames.batch} />
      </StrictMode>,
    );
  });
  let cursor = 0;
  const show = async (entries: ViewEntry[], head: ViewEntry = header()) => {
    cursor += 1;
    transport.deliver(JSON.stringify(event(batch(cursor, [reset(entries, head)]))));
    await act(async () => frames.run());
  };
  const play = async (bodies: ViewPatchBody[]) => {
    transport.deliver(JSON.stringify(event(batch(cursor + 1, bodies))));
    cursor += bodies.length;
    await act(async () => frames.run());
  };
  return { transport, show, play, handed: () => handed };
}

const $ = (id: string) => container.querySelector<HTMLElement>(`[data-testid="${id}"]`);
const all = (id: string) => [...container.querySelectorAll<HTMLElement>(`[data-testid="${id}"]`)];
const click = (el: HTMLElement | null) => act(async () => el!.click());

async function type(text: string) {
  const area = $("composer") as HTMLTextAreaElement;
  // React tracks the value it set; writing through the prototype's setter makes the input event a real change.
  const proto = HTMLTextAreaElement.prototype;
  await act(async () => {
    Reflect.set(proto, "value", text, area);
    area.dispatchEvent(new Event("input", { bubbles: true }));
  });
}

test("on start it reopens the newest matter's last thread, subscribes once under Strict Mode and applies each patch once", async () => {
  const { transport, show, play, handed } = await open();
  expect($("matter")?.textContent).toBe("Lease review");
  expect(transport.only("subscribe")).toEqual([{ call: "subscribe", stream: STREAM, after: null }]);
  await show([userMessage(5, 1, "What does clause nine say?"), agentText(6, 1)]);
  await play([append(1, 0, "Clause nine "), append(1, 12, "covers notice.")]);
  expect(all("message-user").map((e) => e.textContent)).toEqual(["What does clause nine say?"]);
  expect(all("message-agent").map((e) => e.textContent)).toEqual(["Clause nine covers notice."]);
  expect(handed()).toBe(2);
  expect(transport.only("subscribe")).toHaveLength(1);
});

test("new matter picks a folder, creates the matter, then its first thread with agent claude", async () => {
  const commands: string[] = [];
  const { transport } = await open({
    query: (q) => (q === "list_matters" ? { matters: [] } : { threads: [] }),
    pickFolder: () => "folder-1",
    dispatch: ({ command }) => {
      commands.push(JSON.stringify(command));
      if (command.create_matter !== undefined) return { matter_created: { matter: MATTER } };
      return { thread_created: { thread: THREAD } };
    },
  });
  expect($("composer")).toHaveProperty("disabled", true);
  await click($("new-matter"));
  expect(commands).toEqual([
    JSON.stringify({
      create_matter: { name: "Matter", source: { existing_folder: { folder: "folder-1" } } },
    }),
    JSON.stringify({ create_thread: { matter: MATTER, agent: "claude" } }),
  ]);
  expect(transport.only("subscribe").map((s) => s.stream)).toEqual([STREAM]);
  expect($("composer")).toHaveProperty("disabled", false);
});

test("send dispatches the prompt; while the turn runs Send is disabled and Cancel sends cancel", async () => {
  const sent: unknown[] = [];
  const { show } = await open({
    ...REOPEN,
    dispatch: ({ command }) => {
      sent.push(command);
      return command.send_prompt !== undefined
        ? { prompt_accepted: { turn: TURN, message: uuid(1, 5) } }
        : "done";
    },
  });
  await show([]);
  expect($("send")).toHaveProperty("disabled", true);
  await type("Summarise the lease");
  expect($("cancel")).toBeNull();
  await click($("send"));
  expect(sent).toEqual([
    { send_prompt: { thread: THREAD, content: [{ text: { text: "Summarise the lease" } }] } },
  ]);
  // Accepted but no step yet: the turn counts as running.
  await type("next");
  expect($("send")).toHaveProperty("disabled", true);
  await show([], header("running"));
  await click($("cancel"));
  expect(sent.at(-1)).toEqual({ cancel: { thread: THREAD } });
  await show([step(9, { stop: "cancelled" })]);
  expect($("cancel")).toBeNull();
  expect($("send")).toHaveProperty("disabled", false);
  expect($("turn-stopped")?.dataset["stopReason"]).toBe("cancelled");
});

test("permission buttons are labelled by option kind, never the agent's label, and answer through answer_permission", async () => {
  const { transport, show } = await open();
  const kinds = ["allow_once", "allow_always", "reject_once", "reject_always", "unknown"] as const;
  await show(
    [
      {
        permission: {
          order: 7,
          request: "req-1",
          options: kinds.map((kind) => ({
            option_id: `opt-${kind}`,
            kind,
            agent_label: "Click me, agent says",
          })),
          deliverable: "live",
          state: "pending",
        },
      },
    ],
    header("needs_you"),
  );
  const buttons = all("permission").flatMap((p) => [...p.querySelectorAll("button")]);
  expect(buttons.map((b) => [b.dataset["testid"], b.textContent])).toEqual(
    kinds.slice(0, 4).map((k) => [`permission-${k}`, PERMISSION_LABEL[k]]),
  );
  await click($("permission-allow_always"));
  expect(transport.only("answer_permission")).toEqual([
    { call: "answer_permission", thread: THREAD, request: "req-1", option: "opt-allow_always" },
  ]);
});

test("undo sends the turn's files; a limit stop shows its reset time as ISO UTC", async () => {
  const sent: unknown[] = [];
  const { show } = await open({
    ...REOPEN,
    dispatch: ({ command }) => (sent.push(command), "done"),
  });
  const resetsAt = Date.UTC(2026, 10, 1, 9, 30);
  await show([
    step(8, {
      checkpoint_after: "cp-1",
      files: [
        { path: "lease.docx", change: "modified", size_bytes: 10, may_include_lawyer_edits: false },
      ],
    }),
    { limit_stop: { order: 9, turn: uuid(2, 4), resets_at: resetsAt, reason: "plan_window" } },
  ]);
  await click($("undo"));
  expect(sent).toEqual([
    { undo_step: { thread: THREAD, turn: TURN, files: [{ path: "lease.docx", choice: "undo" }] } },
  ]);
  const time = $("limit-resets-at") as HTMLTimeElement;
  expect(time.dateTime).toBe("2026-11-01T09:30:00.000Z");
  expect($("limit-stop")?.contains(time)).toBe(true);
});

test("a failed command shows copy chosen by its code, not its message", async () => {
  await open({
    ...REOPEN,
    dispatch: () => {
      throw { code: "thread.turn_running", message: "internal detail", retryable: true };
    },
  });
  await type("hello");
  await click($("send"));
  expect($("error")?.dataset["code"]).toBe("thread.turn_running");
  expect($("error")?.textContent).toBe(ERROR_COPY["thread.turn_running"]);
  expect(($("composer") as HTMLTextAreaElement).value).toBe("hello");
});
