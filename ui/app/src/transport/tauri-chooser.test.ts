// The desktop transport's pick where Windows' folder window would stall: `pick_folder_or_choose` answers `choose`, the
// session goes to the page's chooser (chooser.ts), its calls name the session, and the pick resolves with what the
// lawyer chose or null. Apart from tauri-transport.test.ts to keep that file under the size limit.
import { afterEach, expect, test, vi } from "vitest";

const answers = new Map<string, (args: unknown) => unknown>();
const invoked: { command: string; args: unknown }[] = [];
vi.mock("@tauri-apps/api/core", () => ({
  isTauri: () => false,
  invoke: async (command: string, args: unknown) => {
    invoked.push({ command, args });
    const answer = answers.get(command);
    if (answer === undefined) return null;
    return answer(args);
  },
  Channel: class {
    onmessage: (m: unknown) => void;
    constructor(onmessage: (m: unknown) => void) {
      this.onmessage = onmessage;
    }
  },
}));

const { TauriTransport } = await import("./tauri-transport.ts");
const { chooserOf } = await import("./chooser.ts");

afterEach(() => {
  answers.clear();
  invoked.length = 0;
});

const settle = () => new Promise((done) => setTimeout(done, 0));
/** The picker's calls, past the channel's own. */
const picks = () =>
  invoked.map((i) => i.command).filter((c) => c.startsWith("pick_") || c.startsWith("chooser_"));

test("choose hands the session to the page's chooser, whose calls name it, and resolves with what it chose", async () => {
  const start = { chooser: 7 };
  const starts = [{ key: 0, kind: "home", display: "" }];
  answers.set("pick_folder_or_choose", () => ({ kind: "choose", start }));
  answers.set("chooser_starts", () => starts);
  answers.set("chooser_list", () => ({
    display: "",
    crumbs: [""],
    state: { kind: "folders", items: [], more: false },
  }));
  answers.set("chooser_choose", () => ({ folder: "folder-2", display: "Documents" }));
  const t = new TauriTransport({ warn: () => {} });
  const store = chooserOf(t);
  const unhost = store.host();
  const picked = t.pickFolder();
  await settle();
  const request = store.current();
  expect(request?.start).toEqual(start);
  await expect(request?.wire.starts()).resolves.toEqual(starts);
  await request?.wire.list({ kind: "start", key: 0 });
  const chosen = await request?.wire.choose();
  if (chosen) request?.chosen(chosen);
  await expect(picked).resolves.toEqual({ folder: "folder-2", display: "Documents" });
  expect(store.current()).toBeNull();
  expect(invoked.filter((i) => i.command.startsWith("chooser_"))).toEqual([
    { command: "chooser_starts", args: { chooser: 7 } },
    { command: "chooser_list", args: { chooser: 7, go: { kind: "start", key: 0 } } },
    { command: "chooser_choose", args: { chooser: 7 } },
  ]);

  // cancelled: the session closed on the desktop, and the pick null
  invoked.length = 0;
  const cancelled = t.pickFolder();
  await settle();
  store.current()?.cancel();
  await expect(cancelled).resolves.toBeNull();
  await settle();
  expect(picks()).toEqual(["pick_folder_or_choose", "chooser_cancel"]);

  // no chooser mounted: the session is closed at once and the pick is null
  unhost();
  invoked.length = 0;
  await expect(t.pickFolder()).resolves.toBeNull();
  expect(picks()).toEqual(["pick_folder_or_choose", "chooser_cancel"]);
});

test("until the host has started only open_stream waits in the IPC: the launch's queries hold back, the pick goes at once", async () => {
  let started!: () => void;
  answers.set("open_stream", () => new Promise<null>((done) => (started = () => done(null))));
  answers.set("pick_folder_or_choose", () => ({ kind: "cancelled" }));
  const t = new TauriTransport({ warn: () => {} });
  const listed = t.query({ list_matters: {} } as never);
  const counted = t.countUi({ onboarding_step: { step: "welcome_shown" } } as never);
  const sent = t.dispatch({ command: { forget_matter: { matter: "m" } } } as never);
  await expect(t.pickFolder()).resolves.toBeNull();
  await settle();
  expect(invoked.map((i) => i.command)).toEqual(["open_stream", "pick_folder_or_choose"]);

  started();
  await Promise.all([listed, counted, sent]);
  expect(invoked.map((i) => i.command)).toEqual([
    "open_stream",
    "pick_folder_or_choose",
    "query",
    "count_ui",
    "dispatch",
  ]);
});

test("the chooser's timing goes to the desktop's log, in milliseconds from the click", async () => {
  answers.set("pick_folder_or_choose", () => ({ kind: "choose", start: { chooser: 3 } }));
  const t = new TauriTransport({ warn: () => {} });
  const store = chooserOf(t);
  const unhost = store.host();
  const now = vi.spyOn(performance, "now");
  now.mockReturnValueOnce(1000).mockReturnValueOnce(1040);
  void t.pickFolder();
  await settle();
  store.current()?.wire.shown?.(1100, 1130);
  expect(invoked.find((i) => i.command === "chooser_shown")?.args).toEqual({
    answerMs: 40,
    renderMs: 100,
    paintMs: 130,
  });
  now.mockRestore();
  store.current()?.cancel();
  unhost();
});
