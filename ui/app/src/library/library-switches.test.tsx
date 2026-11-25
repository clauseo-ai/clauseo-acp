// @vitest-environment happy-dom Skills and connectors' switches, scope and Remove (library L2) in
// the real room over FakeTransport: the head's lens says whose switches the rows show (the matter
// open last, or every matter) and each switch sends set_addon_on or set_addon_scope; each row says
// where it is on; the detail ticks a box per matter; Remove asks first, then sends remove_addon;
// built-ins have no Remove; the firm's lock holds what it holds; a refusal puts the row back and
// says one line. The scripted host answers done and then lists what the test sets.
import { act } from "react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { HostCommand, HostError, QueryResult } from "../transport/bindings.ts";
import type { MatterSummary } from "../transport/protocol.ts";
import { MATTER, uuid } from "../testing/stream-script.ts";
import type { AddonView } from "./addons.ts";
import { mountRoom, THE_MATTER, type Harness } from "../room/thread/harness.ts";
import { SEEN_KEY } from "./seen.ts";

const MEHTA = uuid(2, 2);
const RAO = uuid(3, 2);
const MATTERS: MatterSummary[] = [
  THE_MATTER,
  { matter: MEHTA, name: "Mehta Textiles arbitration", kind: "folder" },
  { matter: RAO, name: "Rao Family Trust", kind: "folder" },
];

let h: Harness | null = null;
beforeEach(() => localStorage.removeItem(SEEN_KEY));
afterEach(async () => {
  await h?.unmount();
  h = null;
  document.body.replaceChildren();
});

const item = (p: Partial<AddonView> & Pick<AddonView, "kind" | "id">): AddonView => ({
  name: p.id,
  description: "A line.",
  source: "added",
  scope: "all",
  on: true,
  status: "ready",
  can_remove: (p.source ?? "added") === "added",
  locked: false,
  switched: [],
  ...p,
});

const BASE: AddonView[] = [
  item({ kind: "skill", id: "legal-drafting", source: "built_in" }),
  item({ kind: "skill", id: "pdf", name: "PDF" }),
  item({ kind: "skill", id: "briefs", name: "Briefs", switched: [MEHTA] }),
  item({ kind: "skill", id: "notice", name: "Notice drafting", scope: { matter: MATTER } }),
  item({ kind: "skill", id: "leases", name: "Leases", scope: { matter: MATTER } }),
  item({ kind: "skill", id: "dormant", name: "Dormant", scope: { matter: MATTER }, switched: [] }),
  item({ kind: "tool", id: "case-law", source: "built_in", status: "needs_setup" }),
  item({ kind: "tool", id: "browser", source: "built_in" }),
  item({ kind: "tool", id: "diary", name: "court-diary", from: { kind: "program", text: "npx" } }),
];
BASE.find((a) => a.id === "notice")!.switched = [MATTER];
BASE.find((a) => a.id === "leases")!.switched = [MATTER, MEHTA];

let items: AddonView[] = BASE;
let sent: HostCommand[] = [];
let refuse: string | null = null;
/** Refuse only a matter's switch (set_addon_on), so every matter's off goes half through. */
let refuseSwitch = false;
const settings = vi.fn();

async function mount(list: AddonView[] = BASE) {
  items = list;
  sent = [];
  refuse = null;
  refuseSwitch = false;
  settings.mockReset();
  h = await mountRoom({
    openSettings: settings,
    matters: MATTERS,
    next: { parts: ["app"], query: () => ({ threads: [] }) },
    query: (query) => {
      if (typeof query === "object" && "list_addons" in query)
        return { addons: { items } } as QueryResult;
      throw new Error(`not scripted: ${JSON.stringify(query)}`);
    },
    dispatch: (command) => {
      // the room's own commands (opening a matter) pass; the place's are recorded
      const ours = ["set_addon_on", "set_addon_scope", "remove_addon"].some((k) => k in command);
      if (!ours) return "done";
      sent.push(command);
      if (refuse !== null) throw { code: refuse, message: "no", retryable: false } as HostError;
      if (refuseSwitch && "set_addon_on" in command)
        throw { code: "internal", message: "no", retryable: false } as HostError;
      return "done";
    },
  });
  await h.settle();
  // the matter open last: Sharma v. State of Delhi
  const rows = [...h.host.querySelectorAll(".room-matters .room-matter-row")];
  await h.click(rows.find((r) => r.textContent?.includes("Sharma v. State of Delhi")));
  await h.click(h.one("library-entry"));
  await act(async () => {
    await vi.dynamicImportSettled();
  });
  await h.settle();
}

/** The host's list changes, and it says so. */
async function listed(next: AddonView[]) {
  items = next;
  await act(async () => h!.transport.appEvent({ addons_changed: {} }));
  await h!.settle();
}

const rowOf = (id: string) =>
  h!.host.querySelector(`[data-id="${id}"]`)?.closest("li, .lib-feature") ?? null;
const switchOf = (id: string) => rowOf(id)?.querySelector<HTMLElement>('[role="switch"]') ?? null;
const checked = (el: Element | null | undefined) => el?.getAttribute("aria-checked") === "true";
const disabled = (el: Element | null | undefined) =>
  el?.hasAttribute("disabled") === true ||
  el?.getAttribute("aria-disabled") === "true" ||
  el?.hasAttribute("data-disabled") === true;
/** The Settings sections opened, in order. */
const opened = () =>
  h!.transport.calls.flatMap((c) => (c.call === "open_settings" ? [c.section] : []));
const text = (id: string) => h!.one(id)?.textContent ?? null;
const tag = (id: string) => rowOf(id)?.querySelector(".lib-row-tag")?.textContent ?? null;
const lensTo = async (name: string) => {
  const lens = h!.one("library-lens")!;
  await h!.click([...lens.querySelectorAll("button")].find((b) => b.textContent === name));
};
const tab = async (name: string) =>
  h!.click([...h!.host.querySelectorAll('[role="tab"]')].find((e) => e.textContent === name));
const pick = (id: string) => h!.click(h!.host.querySelector(`[data-id="${id}"]`));
const buttonNamed = (name: string, root: ParentNode = document) =>
  [...root.querySelectorAll("button")].find((b) => b.textContent === name);

test("the lens: the matter open last first, its switch is that matter's; Every matter's sets the scope", async () => {
  await mount();
  const lens = h!.one("library-lens")!;
  expect(lens.textContent).toBe("Switches forSharma v. State of DelhiEvery matter");
  expect(switchOf("pdf")?.getAttribute("aria-label")).toBe("PDF, on in Sharma v. State of Delhi");
  expect(checked(switchOf("pdf"))).toBe(true);
  // Briefs is off in Mehta only, so on here; Leases on here and in Mehta
  expect(checked(switchOf("briefs"))).toBe(true);
  expect(checked(switchOf("dormant"))).toBe(false);
  await h!.click(switchOf("pdf"));
  expect(sent).toEqual([{ set_addon_on: { kind: "skill", id: "pdf", matter: MATTER, on: false } }]);
  // held as the host will list it until the list follows
  expect(checked(switchOf("pdf"))).toBe(false);
  expect(tag("pdf")).toBe("Every matter but 1");
  await listed(items.map((a) => (a.id === "pdf" ? { ...a, switched: [MATTER] } : a)));
  expect(checked(switchOf("pdf"))).toBe(false);

  await lensTo("Every matter");
  expect(switchOf("notice")?.getAttribute("aria-label")).toBe(
    "Notice drafting, on in every matter",
  );
  expect(checked(switchOf("notice"))).toBe(false);
  sent = [];
  await h!.click(switchOf("notice"));
  expect(sent).toEqual([{ set_addon_scope: { kind: "skill", id: "notice", scope: "all" } }]);
  await listed(BASE);
  // off in every matter: scope to the matter open last, then off there, so it is on nowhere
  sent = [];
  await h!.click(switchOf("briefs"));
  expect(sent).toEqual([
    { set_addon_scope: { kind: "skill", id: "briefs", scope: { matter: MATTER } } },
    { set_addon_on: { kind: "skill", id: "briefs", matter: MATTER, on: false } },
  ]);
  expect(tag("briefs")).toBe("Not used in any matter");
  expect(h!.one("library-explainer")?.textContent).toContain(
    "The switch on each row turns it on or off in every matter.",
  );
});

test("each row says where it is on, from scope and switched", async () => {
  await mount();
  expect(tag("pdf")).toBe("Every matter");
  expect(tag("briefs")).toBe("Every matter but 1");
  expect(tag("notice")).toBe("Only Sharma v. State of Delhi");
  expect(tag("leases")).toBe("Only 2 matters");
  // scope matter, on in no matter: never "Only <matter>"
  expect(tag("dormant")).toBe("Not used in any matter");
  expect(tag("legal-drafting")).toBe("Every matter");
});

test("Where Claude uses it: the choice, and a tick box per matter that switches that matter", async () => {
  await mount();
  await pick("briefs");
  const where = h!.one("library-where")!;
  const ticks = h!.all("library-tick").map((t) => ({
    name: t.querySelector("bdi")?.textContent,
    on: checked(t.querySelector('[role="checkbox"]')),
    last: t.textContent?.endsWith("open last"),
  }));
  expect(ticks).toEqual([
    { name: "Sharma v. State of Delhi", on: true, last: true },
    { name: "Mehta Textiles arbitration", on: false, last: false },
    { name: "Rao Family Trust", on: true, last: false },
  ]);
  const radios = [...where.querySelectorAll('[role="radio"]')];
  expect(radios.map((r) => checked(r))).toEqual([true, false]);
  await h!.click(h!.all("library-tick")[1]!.querySelector('[role="checkbox"]'));
  expect(sent).toEqual([
    { set_addon_on: { kind: "skill", id: "briefs", matter: MEHTA, on: true } },
  ]);
  await listed(BASE);
  sent = [];
  await h!.click(radios[1]);
  expect(sent).toEqual([
    { set_addon_scope: { kind: "skill", id: "briefs", scope: { matter: MATTER } } },
  ]);
  expect(h!.one("library-detail")?.textContent).toContain(
    "Changes apply from each conversation's next message.",
  );
});

test("Remove asks first, then sends remove_addon; Cancel sends nothing", async () => {
  await mount();
  await pick("pdf");
  await h!.click(h!.one("library-remove"));
  const dialog = document.querySelector('[role="alertdialog"]')!;
  expect(dialog.textContent).toContain("Remove PDF?");
  expect(dialog.textContent).toContain(
    "Clauseo deletes its files from this computer and turns it off in every matter. To use it again, add it again.",
  );
  expect(sent).toEqual([]);
  await h!.click(buttonNamed("Cancel", dialog));
  await h!.settle();
  expect(sent).toEqual([]);
  await h!.click(h!.one("library-remove"));
  await h!.click(buttonNamed("Remove", document.querySelector('[role="alertdialog"]')!));
  expect(sent).toEqual([{ remove_addon: { kind: "skill", id: "pdf" } }]);
  // gone while the list catches up, and then as the list has it
  expect(rowOf("pdf")).toBeNull();
  await listed(BASE.filter((a) => a.id !== "pdf"));
  expect(rowOf("pdf")).toBeNull();
});

test("built-ins have no Remove: a built-in skill has no switch; the browser switches per matter only", async () => {
  await mount();
  expect(switchOf("legal-drafting")).toBeNull();
  await pick("legal-drafting");
  expect(h!.one("library-remove")).toBeNull();
  expect(h!.one("library-where")).toBeNull();
  expect(text("library-stays")).toBe("Comes with Clauseo, so it stays on in every matter.");
  await tab("Connectors");
  expect(disabled(switchOf("browser"))).toBe(false);
  await pick("browser");
  expect(h!.one("library-remove")).toBeNull();
  expect(text("library-stays")).toBe(
    "Comes with Clauseo, so it stays. Turn it off where you don't want it.",
  );
  await h!.click(switchOf("browser"));
  expect(sent).toEqual([
    { set_addon_on: { kind: "tool", id: "browser", matter: MATTER, on: false } },
  ]);
  await listed(BASE);
  await lensTo("Every matter");
  expect(disabled(switchOf("browser"))).toBe(true);
  // an added tool is an MCP server, and says so
  expect(h!.host.querySelector(".lib-group-note")?.textContent).toBe("MCP servers");
});

test("the firm's lock: scope matter still switches per matter, but Remove is refused; scope all is on and fixed", async () => {
  const list = BASE.map((a) =>
    a.id === "notice" || a.id === "pdf" ? { ...a, locked: true, can_remove: false } : a,
  );
  await mount(list);
  expect(disabled(switchOf("notice"))).toBe(false);
  expect(disabled(switchOf("pdf"))).toBe(true);
  expect(checked(switchOf("pdf"))).toBe(true);
  expect(rowOf("pdf")?.querySelector('[data-testid="library-row-firm"]')?.textContent).toBe(
    "Set by your firm",
  );
  await h!.click(switchOf("notice"));
  expect(sent).toEqual([
    { set_addon_on: { kind: "skill", id: "notice", matter: MATTER, on: false } },
  ]);
  await listed(list);
  await pick("notice");
  expect(h!.one("library-remove")).toBeNull();
  expect(text("library-stays")).toBe("Your firm decides where this is used and whether it stays.");
  const where = h!.one("library-where")!;
  expect([...where.querySelectorAll('[role="radio"]')].every((r) => disabled(r))).toBe(true);
  expect(disabled(h!.all("library-tick")[0]!.querySelector('[role="checkbox"]'))).toBe(false);
  await lensTo("Every matter");
  expect(disabled(switchOf("notice"))).toBe(true);
});

test.each([
  ["settings.locked", "Your firm decides this for PDF, so nothing changed."],
  ["addon.unknown", "Clauseo couldn't find PDF on this computer, so nothing changed."],
  ["addon.built_in", "PDF comes with Clauseo, so this can't be changed."],
  ["internal", "Clauseo couldn't make that change to PDF. Try again."],
])("a refusal (%s) puts the switch back and says one line", async (code, line) => {
  await mount();
  refuse = code;
  await h!.click(switchOf("pdf"));
  await h!.settle();
  expect(sent).toHaveLength(1);
  expect(checked(switchOf("pdf"))).toBe(true);
  expect(tag("pdf")).toBe("Every matter");
  expect(text("library-refusal")).toBe(line);
  // the next change clears it
  refuse = null;
  await h!.click(switchOf("pdf"));
  expect(h!.one("library-refusal")).toBeNull();
});

test("every matter's off half through: the scope went, the matter's off was refused: says so, and shows what is true", async () => {
  await mount();
  await lensTo("Every matter");
  refuseSwitch = true;
  await h!.click(switchOf("briefs"));
  await h!.settle();
  expect(sent).toHaveLength(2);
  expect(text("library-refusal")).toBe(
    "Briefs is still on in Sharma v. State of Delhi. Clauseo couldn't turn it off there.",
  );
  // on in that one matter: not every matter's, never greyed
  expect(tag("briefs")).toBe("Only Sharma v. State of Delhi");
  expect(checked(switchOf("briefs"))).toBe(false);
  expect(disabled(switchOf("briefs"))).toBe(false);
  // the host's list follows, the same
  await listed(
    BASE.map((a) =>
      a.id === "briefs" ? { ...a, scope: { matter: MATTER }, switched: [MATTER] } : a,
    ),
  );
  expect(tag("briefs")).toBe("Only Sharma v. State of Delhi");
  await lensTo("Sharma v. State of Delhi");
  expect(checked(switchOf("briefs"))).toBe(true);
});

test("built-in connectors under Every matter: the greyed switch says where to switch them off, and Settings opens there", async () => {
  const list = BASE.map((a) => (a.id === "case-law" ? { ...a, status: "ready" as const } : a));
  await mount(list);
  await tab("Connectors");
  expect(h!.all("library-row-note")).toEqual([]);
  await lensTo("Every matter");
  expect(disabled(switchOf("browser"))).toBe(true);
  expect(rowOf("browser")?.querySelector('[data-testid="library-row-note"]')?.textContent).toBe(
    "Comes with Clauseo. Switch it off for a matter from that matter, or turn it off everywhere in Settings › Privacy.",
  );
  const card = h!.one("case-law-card")!;
  expect(card.querySelector('[data-testid="library-row-note"]')?.textContent).toBe(
    "Comes with Clauseo. Switch it off for a matter from that matter, or turn connectors off everywhere in Settings › Connectors.",
  );
  await pick("browser");
  expect(text("library-stays")).toBe(
    "Comes with Clauseo. Switch it off for a matter from that matter, or turn it off everywhere in Settings › Privacy.",
  );
  await h!.click(h!.one("library-open-settings"));
  expect(opened().at(-1)).toBe("privacy");
  await h!.click(card.querySelector(".lib-feature-hit"));
  await h!.click(h!.one("library-open-settings"));
  expect(opened().at(-1)).toBe("connectors");
  expect(sent).toEqual([]);
});
