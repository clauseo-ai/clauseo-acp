// New matter's folder check (folder-check.tsx), over FakeTransport: a folder picked, the form asks FolderSupport with
// the source the create sends. A network drive or a drive without file permissions warns, with Choose another folder
// and Create anyway; create's own refusal shows at once and holds Create back; anything else shows nothing and Create
// goes ahead. Create waits for the answer, at most 5 s; a new pick drops the old answer.
// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { createErrorLine } from "@clauseo/components/clauseo/matter-form-check.ts";
import { settled } from "@clauseo/components/lib/late.ts";
import { fr } from "@clauseo/components/lib/words-folder.ts";
import { afterEach, expect, test, vi } from "vitest";
import type {
  CommandEnvelope,
  CommandOutcome,
  HostError,
  HostQuery,
  MatterSummary,
  PickedFolderSupport,
} from "../transport/protocol.ts";
import { FakeTransport, type FakeHost } from "../transport/fake-transport.ts";
import { uuid } from "../testing/stream-script.ts";
import { Room } from "./room.tsx";
import { connectRoom } from "./start.ts";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement | null = null;
let root: Root | null = null;

afterEach(async () => {
  vi.useRealTimers();
  await act(async () => root?.unmount());
  host?.remove();
  root = null;
  host = null;
});

const A: MatterSummary = { matter: uuid(1, 2), name: "Acme Ltd v. State", kind: "folder" };
const MADE = uuid(5, 2);
const NAME = "Mehta v. Mehta";

/** The check's answer for one pick: a reading, a refusal (thrown), or one the test settles later. */
type Check = PickedFolderSupport | HostError | Promise<PickedFolderSupport>;

/** Today's host, picking `folders` in turn, its FolderSupport answering `checks` in turn. */
function checking(checks: Check[], folders = ["ref-a", "ref-b"]) {
  let list: MatterSummary[] = [A];
  let picks = 0;
  let asked = 0;
  const h: FakeHost = {
    query: (query: HostQuery) => {
      if (typeof query === "object" && "folder_support" in query) {
        const check = checks[asked++] ?? {};
        if ("code" in check) throw check;
        return check instanceof Promise
          ? check.then((support) => ({ folder_support: support }))
          : { folder_support: check };
      }
      return { matters: list };
    },
    dispatch: (command: CommandEnvelope): CommandOutcome => {
      if (!("create_matter" in command.command)) return "done";
      list = [A, { matter: MADE, name: command.command.create_matter.name, kind: "folder" }];
      return { matter_created: { matter: MADE } };
    },
    pickFolder: () => {
      const folder = folders[picks++] ?? "ref-z";
      return { folder, display: `D:\\Matters\\${folder}` };
    },
  };
  return h;
}

async function mount(h: FakeHost) {
  const transport = new FakeTransport(h);
  connectRoom(transport, { language: "en" });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root?.render(<Room transport={transport} />));
  await settle();
  return transport;
}

async function settle() {
  for (let i = 0; i < 3; i += 1)
    await act(async () => new Promise((resolve) => setTimeout(resolve, 0)));
}

const byId = <E extends HTMLElement>(id: string) =>
  document.querySelector<E>(`[data-testid="${id}"]`);
const dialog = () => document.querySelector('[role="dialog"]');
const warning = () => byId("folder-warning");

async function openForm() {
  await act(async () => byId("new-matter")?.click());
  await act(async () => settled());
  const input = byId<HTMLInputElement>("matter-name") as HTMLInputElement;
  await act(async () => {
    Reflect.set(HTMLInputElement.prototype, "value", NAME, input);
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
}

async function press(id: string) {
  await act(async () => byId(id)?.click());
  await settle();
}

const created = (transport: FakeTransport) =>
  transport
    .only("dispatch")
    .map((d) => d.command.command)
    .filter((c) => "create_matter" in c);
const checked = (transport: FakeTransport) =>
  transport
    .only("query")
    .map((q) => q.query)
    .filter((q) => typeof q === "object" && "folder_support" in q);
const existing = (folder: string) => ({
  create_matter: { name: NAME, source: { existing_folder: { folder } } },
});

/** A promise the test settles. */
function later() {
  let settle: (support: PickedFolderSupport) => void = () => {};
  const promise = new Promise<PickedFolderSupport>((resolve) => {
    settle = resolve;
  });
  return { promise, settle };
}

test("a network drive: the warning's title, name, body and why, with Choose another folder and Create anyway in Create's place", async () => {
  const transport = await mount(checking([{ support: "network" }]));
  await openForm();
  await press("matter-existing");
  expect(checked(transport)).toEqual([
    { folder_support: { source: { existing_folder: { folder: "ref-a" } } } },
  ]);
  expect(warning()?.getAttribute("aria-label")).toBe(fr("en", "This folder is on a network drive"));
  expect(warning()?.textContent).toBe(
    fr("en", "On a network drive") +
      fr("en", "Claude asks before every change. Codex can't work here."),
  );
  expect(warning()?.querySelector(".room-info")?.getAttribute("aria-label")).toBe(
    fr("en", "Clauseo can't keep Claude and Codex inside a folder on a network drive."),
  );
  expect(byId("folder-choose")?.textContent).toBe(fr("en", "Choose another folder"));
  expect(byId("folder-anyway")?.textContent).toBe(fr("en", "Create anyway"));
  expect(byId("create-matter")).toBeNull();
  expect(created(transport)).toEqual([]);
});

test("a drive without file permissions: its own title, name and why, the same buttons", async () => {
  await mount(checking([{ support: "no_permissions" }]));
  await openForm();
  await press("matter-existing");
  expect(warning()?.dataset["kind"]).toBe("no_permissions");
  expect(warning()?.getAttribute("aria-label")).toBe(
    fr("en", "This drive doesn't keep file permissions"),
  );
  expect(warning()?.textContent).toContain(fr("en", "No file permissions"));
  expect(warning()?.querySelector(".room-info")?.getAttribute("aria-label")).toBe(
    fr(
      "en",
      "Clauseo can't keep Claude and Codex inside a folder on this drive. USB drives often don't keep permissions.",
    ),
  );
  expect(byId("folder-choose")).not.toBeNull();
  expect(byId("folder-anyway")).not.toBeNull();
});

test("Choose another folder opens the picker again and checks that folder; a fine one takes the warning away", async () => {
  const transport = await mount(checking([{ support: "network" }, {}]));
  await openForm();
  await press("matter-existing");
  await press("folder-choose");
  expect(transport.only("pick_folder")).toHaveLength(2);
  expect(checked(transport)).toHaveLength(2);
  expect(dialog()?.textContent).toContain("D:\\Matters\\ref-b");
  expect(warning()).toBeNull();
  await press("create-matter");
  expect(created(transport)).toEqual([existing("ref-b")]);
});

test("Create anyway makes the matter in the folder warned about", async () => {
  const transport = await mount(checking([{ support: "network" }]));
  await openForm();
  await press("matter-existing");
  await press("folder-anyway");
  expect(created(transport)).toEqual([existing("ref-a")]);
  expect(dialog()).toBeNull();
});

test("create's own refusal: its line shows at once, and Create is held back for that folder", async () => {
  const refused: HostError = { code: "matter.folder_is_home", message: "", retryable: false };
  const transport = await mount(checking([refused]));
  await openForm();
  await press("matter-existing");
  expect(dialog()?.querySelector('[role="alert"]')?.textContent).toBe(
    createErrorLine("en", "matter.folder_is_home", "D:\\Matters\\ref-a"),
  );
  expect(byId<HTMLButtonElement>("create-matter")?.disabled).toBe(true);
  await press("create-matter");
  expect(created(transport)).toEqual([]);
  expect(warning()).toBeNull();
});

test("a check that couldn't answer, a fine folder, one whose permissions can't change, and other errors: no warning, Create goes ahead", async () => {
  const cases: Check[] = [
    { support: "check_failed" },
    {},
    { support: "cant_change_permissions" },
    { code: "command.invalid", message: "", retryable: false },
    { code: "internal", message: "", retryable: false },
  ];
  for (const check of cases) {
    const transport = await mount(checking([check]));
    await openForm();
    await press("matter-existing");
    expect(checked(transport)).toHaveLength(1);
    expect(warning(), JSON.stringify(check)).toBeNull();
    expect(dialog()?.querySelector('[role="alert"]')).toBeNull();
    await press("create-matter");
    expect(created(transport), JSON.stringify(check)).toEqual([existing("ref-a")]);
    await act(async () => root?.unmount());
    host?.remove();
  }
});

test("a new pick drops the old answer: a network drive answered late for the first folder shows nothing", async () => {
  const first = later();
  const transport = await mount(checking([first.promise, {}]));
  await openForm();
  await press("matter-existing");
  await press("matter-existing");
  first.settle({ support: "network" });
  await settle();
  expect(checked(transport)).toHaveLength(2);
  expect(warning()).toBeNull();
  await press("create-matter");
  expect(created(transport)).toEqual([existing("ref-b")]);
});

test("Create waits for the check: a network drive answered meanwhile shows its warning and makes nothing", async () => {
  const slow = later();
  const transport = await mount(checking([slow.promise]));
  await openForm();
  await press("matter-existing");
  await press("create-matter");
  expect(created(transport)).toEqual([]);
  slow.settle({ support: "network" });
  await settle();
  expect(warning()?.dataset["kind"]).toBe("network");
  expect(created(transport)).toEqual([]);
});

test("Create waits at most 5 s for a check that doesn't answer, then makes the matter", async () => {
  const never = new Promise<PickedFolderSupport>(() => {});
  const transport = await mount(checking([never]));
  await openForm();
  await press("matter-existing");
  vi.useFakeTimers();
  await act(async () => byId("create-matter")?.click());
  await act(async () => vi.advanceTimersByTimeAsync(4900));
  expect(created(transport)).toEqual([]);
  await act(async () => vi.advanceTimersByTimeAsync(200));
  vi.useRealTimers();
  await settle();
  expect(created(transport)).toEqual([existing("ref-a")]);
});

test("a parent Create's picker gives on a network drive: the warning, then Create anyway makes it there without asking again", async () => {
  const transport = await mount(checking([{ support: "network" }]));
  await openForm();
  await press("create-matter");
  expect(checked(transport)).toEqual([
    { folder_support: { source: { new_folder: { folder: "ref-a" } } } },
  ]);
  expect(warning()?.dataset["kind"]).toBe("network");
  expect(created(transport)).toEqual([]);
  expect(dialog()?.textContent).toContain(`D:\\Matters\\ref-a\\${NAME}`);
  await press("folder-anyway");
  expect(transport.only("pick_folder")).toHaveLength(1);
  expect(created(transport)).toEqual([
    { create_matter: { name: NAME, source: { new_folder: { folder: "ref-a" } } } },
  ]);
});
