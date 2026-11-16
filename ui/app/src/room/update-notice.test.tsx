// @vitest-environment happy-dom
// R23's update notice in the room (update-notice.tsx), over FakeTransport with the updater scripted (fake-update.ts):
// each state's words and buttons, each button's command (install_update now or after_turn, cancel_scheduled_update),
// After this answer only while a turn runs in some matter, "updated from" once per version, kept across a reload, and
// nothing for idle, checking, downloading, failed or managed. The room sends neither check_for_update nor
// set_update_mode.
import { uw } from "@clauseo/components/lib/words-update.ts";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, test } from "vitest";
import { THREAD } from "../testing/stream-script.ts";
import { FakeTransport } from "../transport/fake-transport.ts";
import { fakeUpdate, type FakeUpdateRefusals } from "../transport/fake-update.ts";
import type {
  ThreadStatus,
  ThreadSummary,
  UpdateState,
  UpdateView,
} from "../transport/protocol.ts";
import { AllThreads } from "./all-threads.ts";
import { anyTurn } from "./turn-running.ts";
import { UpdateNotice } from "./update-notice.tsx";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const MATTER = "00000000-0000-4000-8000-000000000001";
let root: Root | null = null;
let host: HTMLDivElement | null = null;

beforeEach(() => localStorage.clear());
afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  root = null;
  host = null;
});

const view = (state: UpdateState, over: Partial<UpdateView> = {}): UpdateView => ({
  current: "0.0.1",
  state,
  mode: "auto",
  ...over,
});

const READY: UpdateState = { ready: { version: "0.0.2" } };

const thread = (status: ThreadStatus): ThreadSummary => ({
  thread: THREAD,
  matter: MATTER,
  agent: "claude-acp",
  status,
});

async function settle() {
  for (let i = 0; i < 3; i += 1)
    await act(async () => new Promise((resolve) => setTimeout(resolve, 0)));
}

/** The notice over an updater at `shown`, with one thread at `status` in a matter (none when left out). */
async function mount(
  shown: UpdateView,
  { status, refuse }: { status?: ThreadStatus; refuse?: FakeUpdateRefusals } = {},
) {
  const update = fakeUpdate({ view: shown, ...(refuse === undefined ? {} : { refuse }) });
  const transport = new FakeTransport({
    next: {
      query: () => {
        throw new Error("not scripted");
      },
      parts: ["app"],
      first: () => [
        {
          threads_changed: {
            matter: MATTER,
            threads: status === undefined ? [] : [thread(status)],
            removed: [],
            all: true,
          },
        },
      ],
    },
    update,
  });
  const threads = new AllThreads(transport.nextHost());
  threads.start();
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root?.render(<UpdateNotice transport={transport} threads={threads} />));
  await settle();
  return { transport, update, threads };
}

const notice = () => document.querySelector<HTMLElement>(".update-notice");
const button = (id: string) => document.querySelector<HTMLButtonElement>(`[data-testid="${id}"]`);
const buttons = () =>
  [...document.querySelectorAll<HTMLButtonElement>(".update-notice button")].map(
    (b) => b.textContent,
  );
async function press(id: string) {
  const el = button(id);
  if (el === null) throw new Error(`no ${id}`);
  await act(async () => el.click());
  await settle();
}

/** The update commands the transport sent, as their JSON. */
const sent = (transport: FakeTransport) =>
  transport.only("dispatch").map((c) => JSON.stringify(c.command.command));

test("ready in auto mode: the version, when it installs by itself, and Restart to update", async () => {
  await mount(view(READY));
  expect(notice()?.dataset.state).toBe("ready");
  expect(notice()?.textContent).toContain(
    uw("en", "Clauseo {version} is ready to install.", { version: "0.0.2" }),
  );
  expect(notice()?.textContent).toContain(uw("en", "It installs the next time Clauseo opens."));
  expect(buttons()).toEqual(["Restart to update"]);
});

test("ready in notify mode: the same button, with no second line", async () => {
  await mount(view(READY, { mode: "notify" }));
  expect(notice()?.textContent).not.toContain(uw("en", "It installs the next time Clauseo opens."));
  expect(notice()?.querySelector(".rail-notice-line")).toBeNull();
  expect(buttons()).toEqual(["Restart to update"]);
});

test("Restart to update sends install_update now, and the notice follows the host to installing", async () => {
  const { transport } = await mount(view(READY));
  await press("update-restart");
  expect(sent(transport)).toEqual([JSON.stringify({ install_update: { when: "now" } })]);
  expect(notice()?.dataset.state).toBe("installing");
  expect(notice()?.textContent).toBe(uw("en", "Installing the update…"));
  expect(buttons()).toEqual([]);
});

test.each<[string, ThreadStatus]>([
  ["running", "running"],
  ["starting", "starting"],
  ["waiting on the lawyer", "needs_you"],
  ["queued", { queued: { behind: THREAD } }],
])("while a turn is %s: After this answer, and that restarting now stops it", async (_, status) => {
  await mount(view(READY), { status });
  expect(buttons()).toEqual(["After this answer", "Restart to update"]);
  expect(notice()?.textContent).toContain(uw("en", "Restarting now stops the answer in progress."));
});

test("After this answer shows only while a turn runs, and goes once the turn ends", async () => {
  const { transport } = await mount(view(READY), { status: "idle" });
  expect(button("update-after")).toBeNull();
  expect(notice()?.textContent).not.toContain(
    uw("en", "Restarting now stops the answer in progress."),
  );
  const change = (status: ThreadStatus) =>
    act(async () =>
      transport.appEvent({
        threads_changed: { matter: MATTER, threads: [thread(status)], removed: [], all: false },
      }),
    );
  await change("running");
  expect(button("update-after")).not.toBeNull();
  await change("idle");
  expect(button("update-after")).toBeNull();
});

test("After this answer sends install_update after_turn; scheduled says so, and Cancel takes it back", async () => {
  const { transport } = await mount(view(READY), { status: "running" });
  await press("update-after");
  expect(notice()?.dataset.state).toBe("scheduled");
  expect(notice()?.textContent).toContain(
    uw("en", "Clauseo will restart to update after this answer."),
  );
  expect(buttons()).toEqual(["Cancel"]);
  await press("update-cancel");
  expect(sent(transport)).toEqual([
    JSON.stringify({ install_update: { when: "after_turn" } }),
    JSON.stringify({ cancel_scheduled_update: {} }),
  ]);
  expect(notice()?.dataset.state).toBe("ready");
});

test("a pushed update_changed replaces the view", async () => {
  const { transport } = await mount(view("idle"));
  expect(notice()).toBeNull();
  await act(async () => transport.updateChanged(view(READY)));
  expect(notice()?.dataset.state).toBe("ready");
});

test.each<[string, UpdateState]>([
  ["idle", "idle"],
  ["checking", "checking"],
  ["downloading", { downloading: { version: "0.0.2", received: 10 } }],
  ["failed", { failed: { at: 1, stage: "download" } }],
  ["managed", { managed: { by: "policy" } }],
])("nothing in the room while %s", async (_, state) => {
  await mount(view(state));
  expect(notice()).toBeNull();
});

test("after an update: once, with Got it, and dismissed for that version across a reload", async () => {
  const updated = view("idle", { current: "0.0.2", updated_from: "0.0.1" });
  await mount(updated);
  expect(notice()?.dataset.state).toBe("updated");
  expect(notice()?.textContent).toContain(
    uw("en", "Clauseo was updated from {from} to {current}.", { from: "0.0.1", current: "0.0.2" }),
  );
  await press("update-got-it");
  expect(notice()).toBeNull();
  // a new window, as after a reload: still dismissed
  await act(async () => root?.unmount());
  host?.remove();
  await mount(updated);
  expect(notice()).toBeNull();
  // the next update shows again
  await act(async () => root?.unmount());
  host?.remove();
  await mount(view("idle", { current: "0.0.3", updated_from: "0.0.2" }));
  expect(notice()?.dataset.state).toBe("updated");
});

test("the room sends only install_update and cancel_scheduled_update", async () => {
  const { transport } = await mount(view(READY), { status: "running" });
  await press("update-after");
  await press("update-cancel");
  await press("update-restart");
  const names = transport.only("dispatch").map((c) => Object.keys(c.command.command)[0]);
  expect(new Set(names)).toEqual(new Set(["install_update", "cancel_scheduled_update"]));
});

test("a refused install changes nothing shown, and the button can be pressed again", async () => {
  const { transport } = await mount(view(READY), { refuse: { install_update: "command.invalid" } });
  await press("update-restart");
  expect(notice()?.dataset.state).toBe("ready");
  expect(button("update-restart")?.disabled).toBe(false);
  expect(sent(transport)).toHaveLength(1);
});

test("anyTurn reads every matter's threads", () => {
  const lists = new Map([
    ["a", [thread("idle")]],
    ["b", [thread("running")]],
  ]);
  expect(anyTurn({ lists, now: 0 })).toBe(true);
  expect(anyTurn({ lists: new Map([["a", [thread("idle")]]]), now: 0 })).toBe(false);
});
