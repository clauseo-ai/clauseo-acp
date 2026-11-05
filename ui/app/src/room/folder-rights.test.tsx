// @vitest-environment happy-dom
// Folder rights in the room (folder-notice.tsx, folder-reason.tsx), over FakeTransport: the host's folder_support
// gives each matter its notice once until Close, What to tell IT names the folder, a network drive or a drive without
// permissions marks the matter Not protected, and a pending file edit's card in a folder whose permissions can't
// change says why it asks. A folder the host couldn't check says Claude asks before every change, unless the folder
// is missing. "unknown", a newer Clauseo's reading, and no reading show nothing.
import { fr } from "@clauseo/components/lib/words-folder.ts";
import { frc } from "@clauseo/components/lib/words-folder-card.ts";
import { tbt } from "@clauseo/components/lib/words-toolbar.ts";
import { afterEach, expect, test } from "vitest";
import { header, reset, TURN, uuid } from "../testing/stream-script.ts";
import type {
  FolderSupportKind,
  MatterSummary,
  PermissionOption,
  ViewEntry,
} from "../transport/protocol.ts";
import { detailed, fakeNext } from "./fake-next.ts";
import { mountRoom, summary, THE_MATTER, type Harness } from "./thread/harness.ts";

let h: Harness | null = null;
afterEach(async () => {
  await h?.unmount();
  h = null;
  document.body.replaceChildren();
});

const FOLDER = "D:\\Matters\\Rathi Steels";
const matter = (kind: FolderSupportKind | undefined, over: Partial<MatterSummary> = {}) => ({
  ...THE_MATTER,
  folder_display: FOLDER,
  ...(kind === undefined ? {} : { folder_support: kind }),
  ...over,
});

const notice = () => h!.one("folder-notice");
const text = () => notice()?.textContent ?? "";
const info = () => notice()?.querySelector(".room-info")?.getAttribute("aria-label") ?? null;
const unprotected = () => h!.host.querySelector(".np-mark");

test("a folder whose permissions can't change: Clauseo asks before each edit, why behind the info icon, no Not protected", async () => {
  h = await mountRoom({ matters: [matter("cant_change_permissions")] });
  expect(notice()?.getAttribute("aria-label")).toBe(
    fr("en", "Clauseo asks before each edit in this folder"),
  );
  expect(text()).toContain(fr("en", "Clauseo asks before each edit"));
  expect(info()).toBe(
    fr(
      "en",
      "You can change files here, but not this folder's permissions. Clauseo needs them to keep Claude and Codex inside the matter. Some edits won't go through.",
    ),
  );
  expect(h.one("folder-it")?.textContent).toBe(fr("en", "What to tell IT"));
  expect(unprotected()).toBeNull();
});

test("What to tell IT opens the text for IT with the matter's folder in it, and Copy", async () => {
  h = await mountRoom({ matters: [matter("cant_change_permissions")] });
  expect(h.one("folder-it-text")).toBeNull();
  await h.click(h.one("folder-it"));
  expect(h.one("folder-it")?.getAttribute("aria-expanded")).toBe("true");
  const it = h.one("folder-it-text")?.textContent ?? "";
  expect(it).toContain(`on this folder: \u2068${FOLDER}\u2069.`);
  expect(it).toContain("so Claude and Codex, which it runs, can write only inside it.");
  expect(it).toContain(fr("en", "Copy"));
});

test("Close shuts the notice for that matter only, and it stays shut when the matter opens again", async () => {
  const other: MatterSummary = {
    ...matter("cant_change_permissions"),
    matter: uuid(9, 2),
    name: "Rathi Steels v. Northgate Infra",
  };
  h = await mountRoom({
    matters: [matter("cant_change_permissions"), other],
    threads: (m) => [{ ...summary(uuid(m === other.matter ? 9 : 1, 3)), matter: m }],
  });
  const row = (name: string) =>
    h!.all("matter-row").find((r) => r.textContent?.includes(name)) ?? null;
  // the room opens one of the two; Close shuts its notice, the other's stands, and the first's stays shut
  const first = h.one("matter")?.textContent ?? "";
  const second = first === other.name ? THE_MATTER.name : other.name;
  expect(notice()).not.toBeNull();
  await h.click(h.one("folder-close"));
  expect(notice()).toBeNull();
  await h.click(row(second));
  expect(h.one("matter")?.textContent).toBe(second);
  expect(notice()).not.toBeNull();
  await h.click(row(first));
  expect(h.one("matter")?.textContent).toBe(first);
  expect(notice()).toBeNull();
});

test("a network drive: Not protected, and its notice says Claude asks and Codex can't work here", async () => {
  h = await mountRoom({ matters: [matter("network")] });
  expect(notice()?.getAttribute("aria-label")).toBe(
    fr("en", "This matter's folder is on a network drive"),
  );
  expect(text()).toContain(fr("en", "On a network drive."));
  expect(text()).toContain(fr("en", "Claude asks before every change. Codex can't work here."));
  expect(info()).toBe(
    fr(
      "en",
      "Clauseo can't keep Claude and Codex inside this folder. For full protection, move the matter to a folder on this computer.",
    ),
  );
  expect(h.one("folder-it")).toBeNull();
  expect(unprotected()?.textContent).toBe(tbt("en", "Not protected"));
});

test("a drive without file permissions: Not protected, and its notice", async () => {
  h = await mountRoom({ matters: [matter("no_permissions")] });
  expect(notice()?.getAttribute("aria-label")).toBe(
    fr("en", "This matter's folder is on a drive that doesn't keep file permissions"),
  );
  expect(text()).toContain(fr("en", "No file permissions."));
  expect(text()).toContain(fr("en", "Claude asks before every change. Codex can't work here."));
  expect(info()).toContain("USB drives often don't keep permissions.");
  expect(unprotected()?.textContent).toBe(tbt("en", "Not protected"));
});

test("a newer Clauseo's reading, and none, show no notice and no Not protected, where a network drive shows both", async () => {
  for (const kind of ["network", "unknown", undefined] as const) {
    h = await mountRoom({ matters: [matter(kind)] });
    expect(h.one("transcript")).not.toBeNull();
    expect(notice() !== null, String(kind)).toBe(kind === "network");
    expect(unprotected() !== null, String(kind)).toBe(kind === "network");
    await h.unmount();
    h = null;
    document.body.replaceChildren();
  }
});

const opt = (option_id: string, kind: PermissionOption["kind"]): PermissionOption => ({
  option_id,
  kind,
  agent_label: option_id,
});
const edit = (order: number, n: number, kind: "edit" | "execute" = "edit"): ViewEntry[] => [
  {
    tool_call: {
      order,
      started_at: 0,
      tool_call: uuid(n, 7),
      turn: TURN,
      title: kind === "edit" ? "Editing files" : "Run ls",
      kind,
      status: "pending",
      locations: [],
      content: [],
    },
  },
  {
    permission: {
      order: order + 1,
      request: uuid(n, 8),
      tool_call: uuid(n, 7),
      options: [opt("once", "allow_once"), opt("deny", "reject_once")],
      deliverable: "live",
      state: "pending",
    },
  },
];

/** The room for one matter with these entries in its thread, the turn waiting on the lawyer. */
async function cards(kind: FolderSupportKind | undefined, entries: ViewEntry[]) {
  const harness = await mountRoom({ matters: [matter(kind)] });
  harness.deliver([reset(entries, header("needs_you"))]);
  await harness.settle();
  return harness;
}

test("in a folder whose permissions can't change, an Editing files card says why it asks, behind an info icon too", async () => {
  h = await cards("cant_change_permissions", [...edit(2, 1), ...edit(4, 2, "execute")]);
  const [editing, running] = h.all("permission");
  const reason = editing?.querySelector('[data-testid="folder-reason"]');
  expect(reason?.textContent).toBe(frc("en", "Can't set folder permissions"));
  expect(reason?.querySelector(".room-info")?.getAttribute("aria-label")).toBe(
    frc("en", "Clauseo can't change this folder's permissions, so it asks before each edit."),
  );
  // under the title, "Editing files"
  expect(editing?.querySelector(".perm-title")?.nextElementSibling?.contains(reason ?? null)).toBe(
    true,
  );
  // a card for something else gives no folder reason
  expect(running?.querySelector('[data-testid="folder-reason"]')).toBeNull();
});

test("an Editing files card gives the folder reason only where the permissions can't change", async () => {
  for (const kind of [
    "cant_change_permissions",
    "network",
    "no_permissions",
    "unknown",
    undefined,
  ] as const) {
    h = await cards(kind, edit(2, 1));
    expect(h.one("permission")?.dataset["state"]).toBe("pending");
    expect(h.one("folder-reason") !== null, String(kind)).toBe(kind === "cant_change_permissions");
    await h.unmount();
    h = null;
    document.body.replaceChildren();
  }
});

test("a folder the host couldn't check: one line says Claude asks before every change, with no Codex line, info icon or Not protected", async () => {
  h = await mountRoom({ matters: [matter("check_failed")] });
  const line = fr("en", "Clauseo couldn't check this folder, so Claude asks before every change.");
  expect(notice()?.dataset["kind"]).toBe("check_failed");
  expect(notice()?.getAttribute("aria-label")).toBe(line);
  expect(text()).toBe(line);
  expect(text()).not.toContain("Codex");
  expect(info()).toBeNull();
  expect(h.one("folder-it")).toBeNull();
  expect(unprotected()).toBeNull();
});

test("a missing folder wins over a check that couldn't answer: the moved line shows, the notice doesn't", async () => {
  // found, the same matter has its notice
  const found = detailed(matter("check_failed"));
  h = await mountRoom({ next: fakeNext({ matters: () => [found] }) });
  expect(notice()?.dataset["kind"]).toBe("check_failed");
  await h.unmount();
  document.body.replaceChildren();
  const missing = detailed(matter("check_failed"), { folder_state: "missing" });
  h = await mountRoom({ next: fakeNext({ matters: () => [missing] }) });
  expect(h.host.querySelector(".room-moved")).not.toBeNull();
  expect(notice()).toBeNull();
});
