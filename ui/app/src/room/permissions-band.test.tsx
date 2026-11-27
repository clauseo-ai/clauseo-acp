// The permissions band for a lawyer who already has matters (issue 5, #1303): with a row not asked yet, the step stands
// over the room, headed "Before you go on", and the room stays usable; it leaves once every row is allowed, and stays
// while a row was refused; with every row asked already, or none, no band. RC A4: the band in its own slot above the
// work, never over the thread; each row its own Allow; Documents listed not asked again after an update, on a Mac that
// answered it before, gets one quiet line instead.
// @vitest-environment happy-dom
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { beforeEach, expect, test, vi } from "vitest";
import { forgetDocumentsAnswered } from "./first-run/documents-answered.ts";
import type { PermissionRow, PermissionsHost } from "./first-run/permissions-host.ts";
import { A, byId, mountFirstRun, press } from "./first-run/rig.tsx";

const KEYS: PermissionRow = {
  id: "keychain",
  state: "would_ask",
  folder: null,
  settings_path: null,
  note: null,
};
const DOCS: PermissionRow = {
  ...KEYS,
  id: "files_documents",
  state: "not_asked",
  folder: "Documents",
};
/** A host listing `rows`, each ask answered with its row allowed. */
const host = (rows: PermissionRow[]): PermissionsHost & { ask: ReturnType<typeof vi.fn> } => ({
  list: async () => rows,
  ask: vi.fn(async (id: PermissionRow["id"]) => ({
    ...rows.find((r) => r.id === id)!,
    state: "granted" as const,
  })),
});

beforeEach(() => {
  localStorage.clear();
  forgetDocumentsAnswered();
});

test("a lawyer with matters and a row not asked yet sees the band over the room, which stays usable; it leaves once every row is allowed", async () => {
  const fake = host([DOCS, KEYS]);
  await mountFirstRun({ matters: [A], permissions: fake });
  expect(byId("first-screen")).toBeNull();
  const band = () => byId("permissions-band");
  expect(band()?.querySelector("h2")?.textContent).toBe("Before you go on");
  expect(band()?.textContent).toContain("Your keychain");
  // the matter is open under it
  expect(byId("matter")?.textContent).toContain(A.name);
  // Documents allowed: the keychain still waits its Allow, so the band stays
  await press(band()?.querySelector<HTMLElement>('[data-testid="permission-allow"]') ?? null);
  expect(band()?.querySelector('[data-testid="permission-granted"]')?.textContent).toBe("Allowed");
  // the keychain allowed too: every row allowed, the band leaves
  await press(band()?.querySelector<HTMLElement>('[data-testid="permission-allow"]') ?? null);
  expect(fake.ask).toHaveBeenCalledTimes(2);
  expect(band()).toBeNull();
  expect(byId("matter")).not.toBeNull();
});

test("a row refused in this session keeps the band, with its Allow", async () => {
  const fake: PermissionsHost = {
    list: async () => [KEYS],
    ask: async () => ({ ...KEYS, state: "denied" as const }),
  };
  await mountFirstRun({ matters: [A], permissions: fake });
  await press(byId("permission-allow"));
  expect(byId("permissions-band")).not.toBeNull();
  expect(byId("permission-denied")).not.toBeNull();
  expect(byId("permission-allow")).not.toBeNull();
});

test("no band when every row was asked already and none is a refused keychain", async () => {
  await mountFirstRun({
    matters: [A],
    permissions: host([
      { ...DOCS, state: "denied" },
      { ...KEYS, state: "granted" },
    ]),
  });
  expect(byId("matter")).not.toBeNull();
  expect(byId("permissions-band")).toBeNull();
});

test("a keychain refused at an earlier start raises the band, with its Allow", async () => {
  const fake = host([{ ...KEYS, state: "denied" }]);
  await mountFirstRun({ matters: [A], permissions: fake });
  expect(byId("permissions-band")).not.toBeNull();
  expect(byId("permission-denied")).not.toBeNull();
  await press(byId("permission-allow"));
  expect(fake.ask).toHaveBeenCalledWith("keychain");
  expect(byId("permissions-band")).toBeNull();
});

test("no band with no rows", async () => {
  await mountFirstRun({ matters: [A], permissions: host([]) });
  expect(byId("matter")).not.toBeNull();
  expect(byId("permissions-band")).toBeNull();
});

test("the band's Allow answered with a plain Allow (still would_ask): the row says allowed once only, with its Allow", async () => {
  await mountFirstRun({
    matters: [A],
    permissions: { list: async () => [KEYS], ask: async () => KEYS },
  });
  await press(byId("permission-allow"));
  expect(byId("permission-once")?.textContent).toBe(
    "macOS allowed it once only. Press Allow again and choose Always Allow.",
  );
  expect(byId("permission-allow")).not.toBeNull();
});

// #1344's states: "asking" while macOS's dialog is open, and `asked` once this build asked for the row
test("keychain asking (macOS's dialog open): the band shows it waiting, with no second Allow", async () => {
  await mountFirstRun({ matters: [A], permissions: host([{ ...KEYS, state: "asking" }]) });
  expect(byId("permission-waiting")?.textContent).toBe(
    "macOS is asking. Type your Mac's login password, then choose Always Allow.",
  );
  expect(byId("permission-allow")).toBeNull();
});

test("keychain would_ask with asked: the row says allowed once only, with its Allow", async () => {
  await mountFirstRun({ matters: [A], permissions: host([{ ...KEYS, asked: true }]) });
  expect(byId("permission-once")?.textContent).toBe(
    "macOS allowed it once only. Press Allow again and choose Always Allow.",
  );
  expect(byId("permission-allow")).not.toBeNull();
});

test("keychain would_ask, not asked yet: the first ask, its reason saying to choose Always Allow", async () => {
  await mountFirstRun({ matters: [A], permissions: host([KEYS]) });
  expect(byId("permission-once")).toBeNull();
  expect(byId("permissions-band")?.textContent).toContain("Choose Always Allow.");
  expect(byId("permission-allow")).not.toBeNull();
});

test("a state from a newer desktop reads as unknown: the keychain row keeps its Allow", async () => {
  await mountFirstRun({
    matters: [A],
    permissions: host([{ ...KEYS, state: "something_new" as never }]),
  });
  expect(byId("permission-allow")).not.toBeNull();
});

// #1350's review: Not now puts away the keychain row only; the band stays while another row still wants its Allow
test("Documents not asked and the keychain refused: Not now takes the keychain row away, the band stays with Documents' Allow", async () => {
  const fake = host([DOCS, { ...KEYS, state: "denied" }]);
  await mountFirstRun({ matters: [A], permissions: fake });
  await press(byId("permission-not-now"));
  const band = byId("permissions-band");
  expect(band).not.toBeNull();
  expect(band?.textContent).not.toContain("Your keychain");
  expect(byId("permission-not-now")).toBeNull();
  expect(byId("permission-denied")).toBeNull();
  await press(byId("permission-allow"));
  expect(fake.ask).toHaveBeenCalledWith("files_documents");
  // Documents allowed, nothing else wants an Allow: the band leaves
  expect(byId("permissions-band")).toBeNull();
});

test("the keychain refused alone: Not now leaves no band", async () => {
  await mountFirstRun({ matters: [A], permissions: host([{ ...KEYS, state: "denied" }]) });
  await press(byId("permission-not-now"));
  expect(byId("permissions-band")).toBeNull();
  expect(byId("matter")).not.toBeNull();
});

test("the band takes its own slot above the work, never over the thread (RC A4)", async () => {
  await mountFirstRun({ matters: [A], permissions: host([DOCS, KEYS]) });
  const band = byId("permissions-band")!;
  const centre = document.querySelector("main.room-centre")!;
  const work = centre.querySelector(":scope > .room-work")!;
  expect(work).not.toBeNull();
  // a child of the centre's column, before the work: in flow, never inside the work's box or the thread's scroller
  expect(band.parentElement).toBe(centre);
  expect(band.compareDocumentPosition(work) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  expect(band.closest(".room-work, .room-thread")).toBeNull();
  // the thread and the pinned question, when drawn, are inside the work's box, which they are placed in
  const layers = centre.querySelectorAll(".room-thread, .room-pin");
  expect(layers.length).toBeGreaterThan(0);
  for (const layer of layers) expect(work.contains(layer)).toBe(true);
  // its own room in the column: never shrunk under the thread, never positioned over it
  expect(band.classList).toContain("flex-none");
  expect(band.className).not.toMatch(/\b(absolute|fixed|sticky)\b/);
  const sheet = (name: string) =>
    readFileSync(
      join(dirname(fileURLToPath(import.meta.url)), "../../../components/src/clauseo", name),
      "utf8",
    );
  const work_ = /\.room-work \{([^}]*)\}/.exec(sheet("shell.css"))?.[1] ?? "";
  expect(work_).toMatch(/position: relative;/);
  expect(work_).toMatch(/flex: 1;/);
  // the pinned question is placed in the nearest box with a position: the work's, under the band
  expect(sheet("thread.css")).toMatch(/\.room-pin \{\s*position: absolute;\s*top: 0;/);
  // at a phone's width the work's box gives way as the centre's does, so the page's order holds
  expect(sheet("responsive.css")).toMatch(/\.room-centre,\s*\.room-work \{\s*display: contents;/);
});

test("Documents not asked and the keychain would ask: each row has its own Allow, and the keychain's asks for the keychain", async () => {
  const fake = host([DOCS, KEYS]);
  await mountFirstRun({ matters: [A], permissions: fake });
  const allow = (id: string) =>
    document.querySelector<HTMLElement>(
      `[data-testid="permission-row"][data-id="${id}"] [data-testid="permission-allow"]`,
    );
  expect(allow("files_documents")).not.toBeNull();
  expect(allow("keychain")).not.toBeNull();
  await press(allow("keychain"));
  expect(fake.ask).toHaveBeenCalledWith("keychain");
  expect(fake.ask).not.toHaveBeenCalledWith("files_documents");
});

test("Documents answered at an earlier read, listed not asked after an update: one quiet line with Allow, no band; allowed, it goes", async () => {
  // an earlier run read Documents granted, and the window remembers it
  await mountFirstRun({ matters: [A], permissions: host([{ ...DOCS, state: "granted" }]) });
  expect(localStorage.getItem("clauseo.documents-answered")).toBe("1");
  document.body.innerHTML = "";
  forgetDocumentsAnswered();
  // the update: the desktop lists Documents not asked again, the keychain allowed
  const fake = host([DOCS, { ...KEYS, state: "granted" }]);
  await mountFirstRun({ matters: [A], permissions: fake });
  expect(byId("permissions-step")).toBeNull();
  expect(byId("documents-again")?.textContent).toContain(
    "Clauseo was updated. Allow it to open your Documents folder again.",
  );
  expect(byId("documents-again")?.closest(".room-work")).toBeNull();
  await press(byId("documents-again-allow"));
  expect(fake.ask).toHaveBeenCalledWith("files_documents");
  // the granted read clears it
  expect(byId("documents-again")).toBeNull();
  expect(byId("permissions-band")).toBeNull();
});

test("Documents answered before, but the keychain awaits its Allow: the full band, Documents' row with its own Allow", async () => {
  localStorage.setItem("clauseo.documents-answered", "1");
  await mountFirstRun({ matters: [A], permissions: host([DOCS, KEYS]) });
  expect(byId("permissions-step")).not.toBeNull();
  expect(byId("documents-again")).toBeNull();
  expect(document.querySelectorAll('[data-testid="permission-allow"]')).toHaveLength(2);
});

test("never answered on this Mac: Documents not asked raises the full band", async () => {
  await mountFirstRun({ matters: [A], permissions: host([DOCS, { ...KEYS, state: "granted" }]) });
  expect(byId("permissions-step")).not.toBeNull();
  expect(byId("documents-again")).toBeNull();
});
