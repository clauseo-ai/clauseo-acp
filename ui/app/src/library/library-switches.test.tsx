// @vitest-environment happy-dom Skills and connectors' switches, scope and Remove (library L2) in
// the real room over FakeTransport: the head's lens says whose switches the rows show (the matter
// open last, or every matter) and each switch sends set_addon_on or set_addon_scope; each row says
// where it is on; the detail ticks a box per matter; Remove asks first, then sends remove_addon;
// built-ins have no Remove; the firm's lock holds what it holds; a refusal puts the row back and
// says one line. The scripted host answers done and then lists what the test sets.
import { act } from "react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { CommandOutcome, HostCommand, HostError, QueryResult } from "../transport/bindings.ts";
import type { MatterSummary } from "../transport/protocol.ts";
import { MATTER, uuid } from "../testing/stream-script.ts";
import type { AddonView } from "./addons.ts";
import { mountRoom, THE_MATTER, type Harness } from "../room/thread/harness.ts";
import { afterChange } from "./scope-model.ts";
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
/** The read of what a matter gets (list_addons {matter}) fails. */
let failMatterRead = false;
/** The transport itself fails: not a host's refusal. */
let broken = false;
/** The items turned off themselves: off in every matter, switch or not. */
let offItself = new Set<string>();
/** While set, the place's commands wait for it. */
let gate: Promise<void> | null = null;
const settings = vi.fn();

/** `on` as the host resolves it for a matter: an item on nowhere is off there, switch or not. */
function resolved(a: AddonView, matter: string): boolean {
  if (offItself.has(a.id) || a.enabled === false) return false;
  if (a.kind === "skill" && a.source === "built_in") return true;
  if (a.locked && a.scope === "all") return true;
  const switched = (a.switched ?? []).includes(matter);
  return a.scope === "all" ? a.on && !switched : switched;
}

async function mount(
  list: AddonView[] = BASE,
  matters: MatterSummary[] = MATTERS,
  failing = false,
) {
  items = list;
  sent = [];
  refuse = null;
  failMatterRead = failing;
  broken = false;
  offItself = new Set();
  gate = null;
  settings.mockReset();
  h = await mountRoom({
    openSettings: settings,
    matters,
    next: { parts: ["app"], query: () => ({ threads: [] }) },
    query: (query) => {
      if (typeof query === "object" && "list_addons" in query) {
        const matter = query.list_addons.matter;
        if (matter == null) return { addons: { items } } as QueryResult;
        if (failMatterRead) throw { code: "internal", message: "no", retryable: false };
        const here = items.map((a) => ({ ...a, on: resolved(a, matter) }));
        return { addons: { items: here } } as QueryResult;
      }
      throw new Error(`not scripted: ${JSON.stringify(query)}`);
    },
    dispatch: async (command): Promise<CommandOutcome> => {
      // the room's own commands (opening a matter) pass; the place's are recorded
      const ours = ["set_addon_on", "set_addon_scope", "remove_addon"].some((k) => k in command);
      if (!ours) return "done";
      sent.push(command);
      if (gate !== null) await gate;
      if (broken) throw new Error("the pipe closed");
      if (refuse !== null) throw { code: refuse, message: "no", retryable: false } as HostError;
      return "done";
    },
  });
  await h.settle();
  // the matter open last: Sharma v. State of Delhi
  const rows = [...h.host.querySelectorAll(".room-matters .room-matter-row")];
  const sharma = rows.find((r) => r.textContent?.includes("Sharma v. State of Delhi"));
  if (sharma !== undefined) await h.click(sharma);
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
  // every matter at once: one set_addon_on with no matter
  expect(sent).toEqual([{ set_addon_on: { kind: "skill", id: "notice", on: true } }]);
  expect(tag("notice")).toBe("Every matter");
  await listed(BASE);
  sent = [];
  await h!.click(switchOf("briefs"));
  expect(sent).toEqual([{ set_addon_on: { kind: "skill", id: "briefs", on: false } }]);
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

/** The place's commands wait until the returned release is called. */
function holdHost(): () => Promise<void> {
  let release = () => {};
  gate = new Promise((done) => (release = done));
  return async () => {
    gate = null;
    release();
    await h!.settle();
  };
}

const note = (id: string) =>
  rowOf(id)?.querySelector('[data-testid="library-row-note"]')?.textContent ?? null;
const radiosChecked = () =>
  [...h!.one("library-where")!.querySelectorAll('[role="radio"]')].map((r) => checked(r));

test("an item switched on in the matter but turned off itself shows off, says where, and its switch turns it back on", async () => {
  await mount();
  // switched on in Sharma, but turned off itself (D5's enabled)
  await listed(BASE.map((a) => (a.id === "notice" ? { ...a, enabled: false } : a)));
  expect(checked(switchOf("notice"))).toBe(false);
  expect(tag("notice")).toBe("Turned off");
  expect(note("notice")).toBe(
    "Turned off in Settings › Skills, so no matter uses it. Switch it on here to turn it back on.",
  );
  await pick("notice");
  expect(radiosChecked()).toEqual([false, false]);
  expect(h!.all("library-tick")).toEqual([]);
  expect(text("library-own-off")).toBe(
    "Turned off in Settings › Skills, so no matter uses it. Choose where to use it to turn it back on.",
  );
  await h!.click(switchOf("notice"));
  expect(sent).toEqual([
    { set_addon_on: { kind: "skill", id: "notice", matter: MATTER, on: true } },
  ]);
  expect(checked(switchOf("notice"))).toBe(true);
});

test("a tool off everywhere: no choice chosen, no tick boxes, and either choice turns it back on", async () => {
  await mount(BASE.map((a) => (a.id === "diary" ? { ...a, on: false } : a)));
  await tab("Connectors");
  expect(note("diary")).toBe(
    "Turned off in Settings › Tools, so no matter uses it. Switch it on here to turn it back on.",
  );
  await pick("diary");
  expect(radiosChecked()).toEqual([false, false]);
  expect(text("library-own-off")).toBe(
    "Turned off in Settings › Tools, so no matter uses it. Choose where to use it to turn it back on.",
  );
  await h!.click(h!.one("library-where")!.querySelector('[role="radio"]'));
  expect(sent).toEqual([{ set_addon_scope: { kind: "tool", id: "diary", scope: "all" } }]);
});

test("enabled false: the item's own off line and no tick boxes, in every lens", async () => {
  const off = (a: AddonView) =>
    a.id === "leases" || a.id === "browser" ? { ...a, enabled: false } : a;
  await mount(BASE.map(off));
  const line =
    "Turned off in Settings › Skills, so no matter uses it. Switch it on here to turn it back on.";
  // on in Sharma and Mehta by its switches, but turned off itself
  for (const lens of ["Sharma v. State of Delhi", "Every matter"]) {
    await lensTo(lens);
    expect(checked(switchOf("leases"))).toBe(false);
    expect(tag("leases")).toBe("Turned off");
    expect(note("leases")).toBe(line);
    await pick("leases");
    expect(h!.all("library-tick")).toEqual([]);
    expect(radiosChecked()).toEqual([false, false]);
    await pick("leases");
  }
  // the browser is turned on in Settings: its switch greyed, the line, and Settings
  await tab("Connectors");
  for (const lens of ["Sharma v. State of Delhi", "Every matter"]) {
    await lensTo(lens);
    expect(checked(switchOf("browser"))).toBe(false);
    expect(disabled(switchOf("browser"))).toBe(true);
    expect(note("browser")).toBe(
      "Turned off in Settings › Privacy, so no matter uses it. Turn it on there first.",
    );
  }
  await pick("browser");
  expect(h!.all("library-tick")).toEqual([]);
  await h!.click(h!.one("library-open-settings"));
  expect(opened().at(-1)).toBe("privacy");
});

test("a failed read of what the matter gets keeps its last answer; with none yet, says so", async () => {
  await mount();
  // the matter's read says Notice drafting is off there, as only that read can
  offItself = new Set(["notice"]);
  await listed([...BASE]);
  expect(checked(switchOf("notice"))).toBe(false);
  failMatterRead = true;
  await listed([...BASE]);
  expect(checked(switchOf("notice"))).toBe(false);
  expect(h!.one("library-unread")).toBeNull();
  await h!.unmount();
  h = null;
  document.body.replaceChildren();
  // a first read that fails: the switches as set, and one quiet line
  await mount(BASE, MATTERS, true);
  expect(checked(switchOf("notice"))).toBe(true);
  expect(text("library-unread")).toBe(
    "Clauseo couldn't check what Sharma v. State of Delhi gets just now. The switches show how each is set.",
  );
});

test("a switch done, then the matter's re-read fails: the row shows its new state, an unchanged row its last answer", async () => {
  await mount();
  // the matter's read says Notice drafting is off there; Dormant is off there too
  offItself = new Set(["notice"]);
  await listed([...BASE]);
  expect(checked(switchOf("notice"))).toBe(false);
  expect(checked(switchOf("dormant"))).toBe(false);
  failMatterRead = true;
  await h!.click(switchOf("dormant"));
  await h!.settle();
  expect(sent).toEqual([
    { set_addon_on: { kind: "skill", id: "dormant", matter: MATTER, on: true } },
  ]);
  await listed(BASE.map((a) => (a.id === "dormant" ? { ...a, switched: [MATTER] } : a)));
  // Dormant's fields changed since the answer came: the list says on, not the old answer
  expect(checked(switchOf("dormant"))).toBe(true);
  expect(checked(switchOf("notice"))).toBe(false);
  expect(h!.one("library-unread")).toBeNull();
});

test("off in every matter while with the host: the row keeps its scope and reads Not used in any matter", async () => {
  await mount();
  await lensTo("Every matter");
  const release = holdHost();
  await h!.click(switchOf("pdf"));
  expect(checked(switchOf("pdf"))).toBe(false);
  expect(tag("pdf")).toBe("Not used in any matter");
  await pick("pdf");
  expect(h!.one("library-own-off")).toBeNull();
  await release();
  const pdf = BASE.find((a) => a.id === "pdf")!;
  const held = afterChange(pdf, { set_addon_on: { kind: "skill", id: "pdf", on: false } });
  expect(held.scope).toBe("all");
});

test("Every matter from some matters' own choices says it replaces them", async () => {
  await mount();
  await pick("briefs");
  // scope all, off in Mehta: the line sits under the choice not chosen
  const replaces = h!.all("library-replaces");
  expect(replaces.map((r) => r.closest(".lib-choice")?.textContent?.startsWith("Only"))).toEqual([
    true,
  ]);
  expect(replaces[0]?.textContent?.trim()).toBe("This replaces the choices made in each matter.");
  await pick("pdf");
  expect(h!.all("library-replaces")).toEqual([]);
});

test("every matter's lens with no matter at all: off in every matter is still one command", async () => {
  await mount(BASE, []);
  expect(h!.one("library-lens")).toBeNull();
  expect(disabled(switchOf("pdf"))).toBe(false);
  await h!.click(switchOf("pdf"));
  expect(sent).toEqual([{ set_addon_on: { kind: "skill", id: "pdf", on: false } }]);
  expect(tag("pdf")).toBe("Not used in any matter");
});

test("every matter's off refused: the row is back", async () => {
  await mount();
  await lensTo("Every matter");
  refuse = "internal";
  await h!.click(switchOf("briefs"));
  await h!.settle();
  expect(sent).toHaveLength(1);
  expect(checked(switchOf("briefs"))).toBe(true);
  expect(tag("briefs")).toBe("Every matter but 1");
  expect(text("library-refusal")).toBe("Clauseo couldn't make that change to Briefs. Try again.");
});

test("a transport error, not a refusal, puts the row back and says try again", async () => {
  await mount();
  broken = true;
  await h!.click(switchOf("pdf"));
  await h!.settle();
  expect(checked(switchOf("pdf"))).toBe(true);
  expect(tag("pdf")).toBe("Every matter");
  expect(text("library-refusal")).toBe("Clauseo couldn't make that change to PDF. Try again.");
});

test("while a change is with the host its row's controls wait, and a press sends nothing", async () => {
  await mount();
  const release = holdHost();
  await h!.click(switchOf("pdf"));
  expect(sent).toHaveLength(1);
  expect(disabled(switchOf("pdf"))).toBe(true);
  await h!.click(switchOf("pdf"));
  expect(sent).toHaveLength(1);
  await pick("pdf");
  expect(disabled(h!.one("library-remove"))).toBe(true);
  expect(disabled(h!.all("library-tick")[1]!.querySelector('[role="checkbox"]'))).toBe(true);
  // another row is free
  expect(disabled(switchOf("briefs"))).toBe(false);
  await release();
  expect(disabled(switchOf("pdf"))).toBe(false);
  expect(disabled(h!.one("library-remove"))).toBe(false);
});

test("a list read arriving mid-change leaves the row as it will be", async () => {
  await mount();
  const release = holdHost();
  await h!.click(switchOf("pdf"));
  // the host's list from before the change, read while the change is with it
  await listed([...BASE]);
  expect(checked(switchOf("pdf"))).toBe(false);
  await release();
  expect(checked(switchOf("pdf"))).toBe(false);
  await listed(BASE.map((a) => (a.id === "pdf" ? { ...a, switched: [MATTER] } : a)));
  expect(checked(switchOf("pdf"))).toBe(false);
  expect(tag("pdf")).toBe("Every matter but 1");
});

test("two changes in a row: the second leaves the first as it will be", async () => {
  await mount();
  await h!.click(switchOf("pdf"));
  await h!.settle();
  await h!.click(switchOf("leases"));
  await h!.settle();
  expect(sent).toEqual([
    { set_addon_on: { kind: "skill", id: "pdf", matter: MATTER, on: false } },
    { set_addon_on: { kind: "skill", id: "leases", matter: MATTER, on: false } },
  ]);
  expect(checked(switchOf("pdf"))).toBe(false);
  expect(checked(switchOf("leases"))).toBe(false);
  expect(tag("pdf")).toBe("Every matter but 1");
  expect(tag("leases")).toBe("Only Mehta Textiles arbitration");
});

test("after Remove, focus goes to the chosen tab", async () => {
  await mount();
  await pick("pdf");
  // pressed as a lawyer does, so focus is on Remove when the question opens
  const remove = h!.one("library-remove")!;
  act(() => remove.focus());
  await h!.click(remove);
  await h!.click(buttonNamed("Remove", document.querySelector('[role="alertdialog"]')!));
  await h!.settle();
  expect(sent).toEqual([{ remove_addon: { kind: "skill", id: "pdf" } }]);
  const focused = document.activeElement;
  expect(focused?.getAttribute("role")).toBe("tab");
  expect(focused?.textContent).toBe("Skills");
});
