// The first screen's permissions step (issue 5): absent when nothing is needed; the Mac's rows in order, each asked
// only by its own press, one host call per press; waiting while macOS asks, then allowed or refused with where to change
// it; Windows' Controlled folder access blocking, with a matters folder outside Documents as its action and the
// desktop's settings_path and note as detail. The room over a FakeTransport with a fake permissions host.
// @vitest-environment happy-dom
import { expect, test, vi } from "vitest";
import type { PermissionId, PermissionRow, PermissionsHost } from "./permissions-host.ts";
import { allById, byId, mountFirstRun, press } from "./rig.tsx";

const NONE = { folder: null, settings_path: null, note: null };
const DOCS: PermissionRow = {
  ...NONE,
  id: "files_documents",
  state: "not_asked",
  folder: "Documents",
};
const KEYS: PermissionRow = { ...NONE, id: "keychain", state: "would_ask" };
const MAC_PATH = "System Settings › Privacy & Security › Files and Folders › Clauseo";
const WIN_PATH =
  "Windows Security › Virus & threat protection › Ransomware protection › Allow an app through Controlled folder access";

/** A fake host: `rows` listed, each ask answered by `answer` (held open when it returns a promise never settled). */
function fakeHost(
  rows: PermissionRow[],
  answer: (id: PermissionId) => PermissionRow | Promise<PermissionRow> = (id) => ({
    ...rows.find((r) => r.id === id)!,
    state: "granted",
  }),
) {
  const ask = vi.fn(async (id: PermissionId) => answer(id));
  const list = vi.fn(async () => rows);
  const host: PermissionsHost = { list, ask };
  return { host, ask, list };
}

const shown = () =>
  allById("permission-row").map((r) => [r.getAttribute("data-id"), r.getAttribute("data-shows")]);
const row = (id: PermissionId) =>
  document.querySelector(`[data-testid="permission-row"][data-id="${id}"]`);
const allowIn = (id: PermissionId) =>
  row(id)?.querySelector<HTMLElement>('[data-testid="permission-allow"]') ?? null;

test("the step is absent when nothing is needed: everything allowed, Windows with nothing blocking, or no host", async () => {
  await mountFirstRun({
    matters: [],
    permissions: fakeHost([
      { ...DOCS, state: "granted" },
      { ...KEYS, state: "granted" },
    ]).host,
  });
  expect(byId("first-screen")).not.toBeNull();
  expect(byId("permissions-step")).toBeNull();
});

test("no rows on Windows: no step", async () => {
  await mountFirstRun({ matters: [], platform: "windows", permissions: fakeHost([]).host });
  expect(byId("first-screen")).not.toBeNull();
  expect(byId("permissions-step")).toBeNull();
});

test("mounting the step on the Mac only reads: nothing is asked without a press", async () => {
  const { host, ask, list } = fakeHost([DOCS, KEYS]);
  await mountFirstRun({ matters: [], permissions: host });
  expect(shown()).toEqual([
    ["files_documents", "ask"],
    ["keychain", "ask"],
  ]);
  expect(list).toHaveBeenCalledTimes(1);
  expect(ask).not.toHaveBeenCalled();
});

test("mounting the step on Windows only reads: the blocking row needs no ask for its detail", async () => {
  const blocking: PermissionRow = {
    ...NONE,
    id: "controlled_folder_access",
    state: "blocking",
    settings_path: WIN_PATH,
  };
  const { host, ask, list } = fakeHost([blocking]);
  await mountFirstRun({ matters: [], platform: "windows", permissions: host });
  expect(shown()).toEqual([["controlled_folder_access", "blocking"]]);
  expect(byId("permission-path")?.textContent).toContain("In Windows Security:");
  expect(list).toHaveBeenCalledTimes(1);
  expect(ask).not.toHaveBeenCalled();
});

test("a window with no permissions host shows no step", async () => {
  await mountFirstRun({ matters: [] });
  expect(byId("first-screen")).not.toBeNull();
  expect(byId("permissions-step")).toBeNull();
});

test("the Mac's rows in order, Documents then the keychain, with their reasons; each has its own Allow", async () => {
  const { ask } = fakeHost([]);
  await mountFirstRun({ matters: [], permissions: fakeHost([KEYS, DOCS]).host });
  const step = byId("permissions-step");
  expect(step?.textContent).toContain("Before your first matter");
  expect(shown()).toEqual([
    ["files_documents", "ask"],
    ["keychain", "ask"],
  ]);
  expect(row("files_documents")?.textContent).toContain("Your Documents folder");
  expect(byId("permission-folder")).toBeNull();
  expect(row("keychain")?.textContent).toContain("login password");
  expect(allowIn("keychain")).not.toBeNull();
  // nothing is asked before a press
  expect(ask).not.toHaveBeenCalled();
});

test("each press asks the host once; waiting while macOS asks, a second press does nothing, then Allowed", async () => {
  let answer: (r: PermissionRow) => void = () => {};
  const { host, ask } = fakeHost(
    [DOCS, KEYS],
    () =>
      new Promise<PermissionRow>((resolve) => {
        answer = resolve;
      }),
  );
  await mountFirstRun({ matters: [], permissions: host });
  expect(ask).not.toHaveBeenCalled();
  const allow = allowIn("files_documents");
  await press(allow);
  expect(ask).toHaveBeenCalledTimes(1);
  expect(ask).toHaveBeenCalledWith("files_documents");
  expect(shown()[0]).toEqual(["files_documents", "waiting"]);
  expect(byId("permission-waiting")?.textContent).toBe(
    "macOS is asking. Choose Allow in its window.",
  );
  // the rest of the screen stays pressable while macOS asks
  expect(byId<HTMLButtonElement>("open-folder")?.disabled).toBe(false);
  await press(allow);
  expect(ask).toHaveBeenCalledTimes(1);
  answer({ ...DOCS, state: "granted" });
  await press(byId("permissions-step"));
  expect(shown()).toEqual([
    ["files_documents", "granted"],
    ["keychain", "ask"],
  ]);
  expect(row("files_documents")?.textContent).toContain("Allowed");
  expect(allowIn("files_documents")).toBeNull();
  answer = () => {};
  await press(allowIn("keychain"));
  expect(ask).toHaveBeenCalledTimes(2);
  expect(ask).toHaveBeenLastCalledWith("keychain");
  expect(byId("permission-waiting")?.textContent).toContain("Always Allow");
});

test("a row already allowed shows Allowed and is never asked; the other row is the one to ask", async () => {
  const { host, ask } = fakeHost([{ ...DOCS, state: "granted" }, KEYS]);
  await mountFirstRun({ matters: [], permissions: host });
  expect(shown()).toEqual([
    ["files_documents", "granted"],
    ["keychain", "ask"],
  ]);
  expect(allowIn("files_documents")).toBeNull();
  expect(ask).not.toHaveBeenCalled();
});

test("refused: Not allowed, where to change it as the desktop says, and System Settings opens", async () => {
  const { host } = fakeHost([DOCS], () => ({ ...DOCS, state: "denied", settings_path: MAC_PATH }));
  const transport = await mountFirstRun({ matters: [], permissions: host });
  await press(allowIn("files_documents"));
  expect(shown()).toEqual([["files_documents", "denied"]]);
  expect(byId("permission-denied")?.textContent).toBe(
    `Not allowed. You can allow it in ${MAC_PATH}.`,
  );
  const opened = vi.spyOn(transport, "openPrivacySettings");
  await press(byId("permission-settings"));
  expect(opened).toHaveBeenCalledTimes(1);
});

test("Documents refused on the Mac: the matters-folder choice beside the ways on, and its press runs the choice", async () => {
  const choose = vi.fn(async () => ({ done: "/Users/asha/Matters" }));
  const { host } = fakeHost([DOCS], () => ({ ...DOCS, state: "denied", settings_path: MAC_PATH }));
  await mountFirstRun({ matters: [], permissions: { ...host, chooseMattersFolder: choose } });
  // not before a refusal: a row still to ask has only its Allow
  expect(byId("folder-choice")).toBeNull();
  await press(allowIn("files_documents"));
  expect(shown()).toEqual([["files_documents", "denied"]]);
  expect(byId("permission-settings")).not.toBeNull();
  expect(byId("folder-choice")?.textContent).toBe("Choose a matters folder outside Documents");
  await press(byId("folder-choice"));
  expect(choose).toHaveBeenCalledTimes(1);
  expect(byId("folder-choice-said")?.textContent).toBe(
    "New matters now go in /Users/asha/Matters. Matters you already have stay where they are.",
  );
});

test("Documents refused, the picker cancelled: no line, and the choice stays for another try", async () => {
  const choose = vi.fn(async () => null);
  const { host } = fakeHost([DOCS], () => ({ ...DOCS, state: "denied", settings_path: MAC_PATH }));
  await mountFirstRun({ matters: [], permissions: { ...host, chooseMattersFolder: choose } });
  await press(allowIn("files_documents"));
  await press(byId("folder-choice"));
  expect(choose).toHaveBeenCalledTimes(1);
  expect(byId("folder-choice-said")).toBeNull();
  expect(byId("folder-choice")?.hasAttribute("disabled")).toBe(false);
});

test("Documents refused where the host can't set a matters folder: no choice", async () => {
  const { host } = fakeHost([DOCS], () => ({ ...DOCS, state: "denied", settings_path: MAC_PATH }));
  await mountFirstRun({ matters: [], permissions: host });
  await press(allowIn("files_documents"));
  expect(byId("permission-settings")).not.toBeNull();
  expect(byId("folder-choice")).toBeNull();
});

test("the keychain refused: it can be allowed here at any time, its Allow stays, and the items are said plainly", async () => {
  // the desktop's note as permissions.rs writes it: "service (account)", joined with ", "
  const note = "ai.clauseo.desktop.connectors (westlaw), ai.clauseo.desktop.connectors (manupatra)";
  const { host, ask } = fakeHost([KEYS], () => ({ ...KEYS, state: "denied", note }));
  await mountFirstRun({ matters: [], permissions: host });
  await press(allowIn("keychain"));
  expect(shown()).toEqual([["keychain", "denied"]]);
  expect(byId("permission-denied")?.textContent).toBe(
    "Not allowed. You can allow it in Settings, under Privacy, at any time.",
  );
  expect(byId("permission-note")?.textContent).toBe(
    "Without it, Clauseo can't read your saved sign-ins for connectors.",
  );
  // never the raw note
  expect(byId("permissions-step")?.textContent).not.toContain("ai.clauseo");
  expect(byId("permission-settings")).toBeNull();
  await press(allowIn("keychain"));
  expect(ask).toHaveBeenCalledTimes(2);
});

test("the keychain refused with the lawyer's own connectors' keys among its items: named for what they are", async () => {
  const note = "ai.clauseo.desktop.mcp (court-filings), ai.clauseo.desktop (claude-acp)";
  const { host } = fakeHost([KEYS], () => ({ ...KEYS, state: "denied", note }));
  await mountFirstRun({ matters: [], permissions: host });
  await press(allowIn("keychain"));
  expect(byId("permission-note")?.textContent).toBe(
    "Without it, Clauseo can't read your saved sign-ins for Claude or ChatGPT, or the saved keys for your own connectors.",
  );
  expect(byId("permissions-step")?.textContent).not.toContain("ai.clauseo");
});

test("the keychain refused with only the lawyer's own connectors' keys", async () => {
  const note = "ai.clauseo.desktop.mcp (court-filings)";
  const { host } = fakeHost([KEYS], () => ({ ...KEYS, state: "denied", note }));
  await mountFirstRun({ matters: [], permissions: host });
  await press(allowIn("keychain"));
  expect(byId("permission-note")?.textContent).toBe(
    "Without it, Clauseo can't read the saved keys for your own connectors.",
  );
});

test("a keychain row its silent check couldn't read (unknown) counts as not asked: it has Allow", async () => {
  const { host, ask } = fakeHost([{ ...KEYS, state: "unknown" }]);
  await mountFirstRun({ matters: [], permissions: host });
  expect(shown()).toEqual([["keychain", "ask"]]);
  await press(allowIn("keychain"));
  expect(ask).toHaveBeenCalledWith("keychain");
  expect(shown()).toEqual([["keychain", "granted"]]);
});

test("a Check again that fails keeps the rows: the step never vanishes", async () => {
  const denied: PermissionRow = { ...DOCS, state: "denied", settings_path: MAC_PATH };
  let reads = 0;
  const host: PermissionsHost = {
    list: async () => {
      reads += 1;
      if (reads > 1) throw new Error("couldn't read");
      return [denied];
    },
    ask: async () => denied,
  };
  await mountFirstRun({ matters: [], permissions: host });
  expect(shown()).toEqual([["files_documents", "denied"]]);
  await press(byId("permission-check"));
  expect(reads).toBe(2);
  expect(shown()).toEqual([["files_documents", "denied"]]);
});

test("an ask that fails says so, and its Allow stays for another try", async () => {
  const { host, ask } = fakeHost([DOCS], () => Promise.reject(new Error("no")));
  await mountFirstRun({ matters: [], permissions: host });
  await press(allowIn("files_documents"));
  expect(byId("permission-failed")?.textContent).toBe("Clauseo couldn't ask macOS. Try again.");
  await press(allowIn("files_documents"));
  expect(ask).toHaveBeenCalledTimes(2);
});

test("Windows, Controlled folder access blocking: said plainly, a matters folder outside Documents, the desktop's detail", async () => {
  const blocking: PermissionRow = {
    ...NONE,
    id: "controlled_folder_access",
    state: "blocking",
    folder: "Documents\\Clauseo",
  };
  const note = "Clauseo's helper programs need allowing too.";
  const { host, ask } = fakeHost([{ ...blocking, settings_path: WIN_PATH, note }]);
  const transport = await mountFirstRun({ matters: [], platform: "windows", permissions: host });
  expect(shown()).toEqual([["controlled_folder_access", "blocking"]]);
  expect(byId("permission-folder")?.textContent).toBe("Documents\\Clauseo");
  expect(row("controlled_folder_access")?.textContent).toContain(
    "Windows is blocking your matters folder",
  );
  // the advice is the action; where the setting lives and the note are the desktop's detail, as given
  expect(byId("permission-other-folder")?.textContent).toBe(
    "Choose a matters folder outside Documents",
  );
  expect(byId("permission-path")?.textContent).toBe(
    "In Windows Security: Virus & threat protection › Ransomware protection › Allow an app through Controlled folder access",
  );
  expect(byId("permission-note")?.textContent).toBe(note);
  // nothing for the lawyer to allow here: no Allow, no macOS lead
  expect(allowIn("controlled_folder_access")).toBeNull();
  expect(byId("permissions-step")?.textContent).not.toContain("macOS");
  expect(ask).not.toHaveBeenCalled();
  // the first screen's own Open the folder: the picker (cancelled here)
  const picks = vi.spyOn(transport, "pickFolder").mockResolvedValue(null);
  await press(byId("permission-other-folder"));
  expect(picks).toHaveBeenCalledTimes(1);
});

test("Windows where the host can set a matters folder: the row's action is that choice, and the picker for the first matter goes", async () => {
  const blocking: PermissionRow = { ...NONE, id: "controlled_folder_access", state: "blocking" };
  const choose = vi.fn(async () => ({ refused: "store.io" as const }));
  const { host } = fakeHost([blocking], () => blocking);
  await mountFirstRun({
    matters: [],
    platform: "windows",
    permissions: { ...host, chooseMattersFolder: choose },
  });
  expect(byId("permission-other-folder")).toBeNull();
  await press(byId("folder-choice"));
  expect(choose).toHaveBeenCalledTimes(1);
  expect(byId("folder-choice-said")?.textContent).toBe(
    "Clauseo couldn't save the change. Try again.",
  );
});

test("Windows: a note that repeats the row's reason is not printed twice", async () => {
  const why =
    "Windows blocks the assistants' tools, such as PowerShell, from saving in protected folders. Choose a matters folder outside Documents, Desktop and Pictures, or ask your IT team.";
  const blocking: PermissionRow = { ...NONE, id: "controlled_folder_access", state: "blocking" };
  const { host } = fakeHost([blocking], () => ({
    ...blocking,
    settings_path: WIN_PATH,
    note: why,
  }));
  await mountFirstRun({ matters: [], platform: "windows", permissions: host });
  expect(byId("permission-note")).toBeNull();
  expect(row("controlled_folder_access")?.textContent?.split(why).length).toBe(2);
});
