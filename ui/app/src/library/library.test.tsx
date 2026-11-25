// @vitest-environment happy-dom Skills and connectors in the real room over FakeTransport (library
// L1, read-only): the rail's row opens the place in the centre with the wall hidden; Skills lists
// the built-ins and the added skill packs with where each is on; Connectors leads with Clauseo Case
// Law in each of its states, then the Web browser, then the lawyer's own MCP servers; the side
// panel explains the part until a row is picked, then shows that item; the words name the
// assistant. Opening a matter closes the place. The list comes from list_addons over a scripted
// host that lists as the real one does (Case Law's status from the connectors' state, left out
// until first checked) and says addons_changed after each change.
import { act } from "react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { FakeTransport } from "../transport/fake-transport.ts";
import type { AddonStatus, QueryResult } from "../transport/bindings.ts";
import type {
  ConnectorStatus,
  ConnectorsState,
  McpToolsView,
  PackStatus,
} from "../transport/protocol.ts";
import type { AddonView } from "./addons.ts";
import { mountRoom, type Harness } from "../room/thread/harness.ts";
import { SEEN_KEY } from "./seen.ts";
import {
  permissionsHostOf,
  providePermissions,
  type PermissionRow,
} from "../room/first-run/permissions-host.ts";

let h: Harness | null = null;
beforeEach(() => localStorage.removeItem(SEEN_KEY));
afterEach(async () => {
  await h?.unmount();
  h = null;
  document.body.replaceChildren();
});

const GIVES =
  "Find and read Supreme Court, High Court and tribunal judgments and central Acts, and see who cites each judgment.";

const PACKS: PackStatus[] = [
  { id: "notice-drafting", name: "Notice drafting", state: "on", added_here: true },
  { id: "lease-review", name: "Lease review", state: "changed", added_here: true },
  { id: "old-briefs", name: "Old briefs", state: "off", added_here: true },
];

const TOOLS: McpToolsView = {
  control: "lawyer",
  importable: [],
  servers: [
    {
      name: "court-diary",
      kind: "local",
      command: "npx",
      args: ["-y", "court-diary-mcp"],
      env: [{ name: "DIARY_TOKEN", saved: true }],
      headers: [],
      enabled: true,
    },
    {
      name: "firm-precedents",
      kind: "remote",
      url: "https://precedents.raoandco.in/mcp",
      args: [],
      env: [],
      headers: [{ name: "Authorization", saved: false }],
      enabled: true,
    },
  ],
} as McpToolsView;

const caseLaw = (
  status: ConnectorStatus | null,
  available = true,
  locked = !available,
): ConnectorsState => ({
  available,
  locked,
  connectors:
    status === null
      ? []
      : [
          {
            id: "case-law",
            name: "Indian Kanoon",
            price_paise: 23_600,
            packs: [{ pack: "p2000", credits: 2_000, paise: 23_600 }],
            status,
          },
        ],
});

const settings = vi.fn();

/** Case Law's status as the host gives it from the connectors' state (stage D's order). */
function hostStatus(state: ConnectorsState): AddonStatus {
  const s = state.connectors.find((c) => c.id === "case-law")?.status;
  if (s === undefined) return "unreachable";
  if (s === "signing_in") return "signing_in";
  if (s === "signed_out" || s === "key_revoked") return "signed_out";
  if (typeof s === "object" && "signed_in" in s) return "ready";
  if (typeof s === "object" && "out_of_credit" in s) return "needs_credit";
  return "needs_setup";
}

/** What the scripted host lists: Case Law left out until a connectors' state, unless set. */
type World = { packs: PackStatus[]; tools: McpToolsView | null; caseLaw: AddonStatus | null };
let world: World = { packs: PACKS, tools: null, caseLaw: null };
/** The list's reads without a matter. */
let reads = 0;

const row0 = (kind: AddonView["kind"], id: string, source: AddonView["source"]): AddonView => ({
  kind,
  id,
  name: id,
  description: "",
  source,
  scope: "all",
  on: true,
  status: "ready",
  can_remove: source === "added",
  locked: false,
});

function hostList(w: World): AddonView[] {
  const builtIns = ["legal-drafting", "legal-research", "matter-brief", "stetdoc"];
  return [
    ...builtIns.map((id) => row0("skill", id, "built_in")),
    ...w.packs.map((p) => ({
      ...row0("skill", p.id, "added"),
      name: p.name ?? p.id,
      from: { kind: "folder" as const, text: p.id },
      on: p.state === "on",
      status: (p.state === "changed" ? "changed" : "ready") as AddonStatus,
    })),
    ...(w.caseLaw === null ? [] : [{ ...row0("tool", "case-law", "built_in"), status: w.caseLaw }]),
    row0("tool", "browser", "built_in"),
    ...(w.tools?.servers ?? []).map((s) => ({
      ...row0("tool", s.name, "added"),
      from:
        s.kind === "remote"
          ? { kind: "address" as const, text: s.url ?? "" }
          : { kind: "program" as const, text: s.command ?? "" },
      status: (s.headers.some((v) => !v.saved) ? "needs_secret" : "ready") as AddonStatus,
    })),
  ];
}

async function mount(packs: PackStatus[] = PACKS) {
  settings.mockReset();
  world = { packs, tools: null, caseLaw: null };
  reads = 0;
  h = await mountRoom({
    next: { parts: ["app"], query: () => ({ threads: [] }) },
    query: (query) => {
      if (typeof query === "object" && "list_addons" in query) {
        // the list's reads; the matter open last's own read follows each (here-read.ts)
        if (query.list_addons.matter == null) reads += 1;
        return { addons: { items: hostList(world) } } as QueryResult;
      }
      throw new Error(`not scripted: ${JSON.stringify(query)}`);
    },
    openSettings: settings,
  });
  await h.settle();
  return h;
}

/** Plays an app event, the list changing with it as the host's would; `caseLaw` sets Case Law's
 * status where the connectors' state can't say it (not offered). */
const play = async (
  event: Parameters<FakeTransport["appEvent"]>[0],
  caseLaw?: AddonStatus | null,
) => {
  if ("connectors_changed" in event) world.caseLaw = hostStatus(event.connectors_changed);
  if ("mcp_tools_changed" in event) world.tools = event.mcp_tools_changed;
  if (caseLaw !== undefined) world.caseLaw = caseLaw;
  await act(async () => h!.transport.appEvent(event));
  await act(async () => h!.transport.appEvent({ addons_changed: {} }));
  await h!.settle();
};

/** Opens the place from the rail, its chunk loaded. */
async function open() {
  await h!.click(h!.one("library-entry"));
  await act(async () => {
    await vi.dynamicImportSettled();
  });
  await h!.settle();
}

const tab = async (name: string) => {
  const t = [...h!.host.querySelectorAll('[role="tab"]')].find((e) => e.textContent === name);
  await h!.click(t);
};

const text = (id: string) => h!.one(id)?.textContent ?? null;
const rows = () =>
  h!.all("library-row").map((r) => ({
    name: r.querySelector(".lib-row-name")?.textContent,
    line: r.querySelector(".lib-row-line")?.textContent,
    tag: r.querySelector(".lib-row-tag")?.textContent,
  }));
const row = (name: string) =>
  h!.all("library-row").find((r) => r.querySelector(".lib-row-name")?.textContent === name);

test("the rail's row counts what the lawyer has, once every source is heard, says New until opened, and opens the place with the wall hidden", async () => {
  await mount();
  // no count until the MCP servers and the connectors' state have both been heard: it never jumps
  expect(h!.one("library-entry")?.textContent).toBe("Skills and connectorsNew");
  await play({ mcp_tools_changed: TOOLS });
  expect(h!.one("library-entry")?.textContent).toBe("Skills and connectorsNew");
  await play({ connectors_changed: caseLaw("not_added") });
  // the skills listed, and the connectors that are the lawyer's: not Case Law unadded, nor the browser
  const entry = h!.one("library-entry")!;
  expect(entry.textContent).toBe("Skills and connectors7 skills, 2 connectorsNew");
  expect(h!.one("library")).toBeNull();
  await open();
  expect(h!.one("library")).not.toBeNull();
  expect(text("matter")).toBe("Skills and connectors");
  expect(h!.host.querySelector(".room-window")?.getAttribute("data-wall")).toBe("hidden");
  expect(h!.one("wall-toggle")).toBeNull();
  expect(h!.one("library-entry")?.getAttribute("aria-current")).toBe("page");
  expect(h!.one("library-entry")?.textContent).toBe("Skills and connectors7 skills, 2 connectors");
  // the rail's row may fold away: the chosen tab has the focus, and controls the panel shown
  expect(document.activeElement?.textContent).toBe("Skills");
  expect(h!.host.querySelector('[role="tabpanel"] .lib-list')).not.toBeNull();
  expect(localStorage.getItem(SEEN_KEY)).toBe("1");
  const tabs = [...h!.host.querySelectorAll('[role="tab"]')].map((t) => t.textContent);
  expect(tabs).toEqual(["Skills", "Connectors"]);
});

test("Skills lists the built-ins, then the added skills, each with where it is on or what is in its way", async () => {
  await mount();
  await open();
  expect(rows()).toEqual([
    {
      name: "Legal drafting",
      line: "Drafts and revises notices, replies, opinions and agreements from the matter's papers, and checks every citation before handing the draft over.",
      tag: "Every matter",
    },
    {
      name: "Legal research",
      line: "Researches Indian law over several searches, reads the later judgments that cite a case, and ties every quote to a page it read.",
      tag: "Every matter",
    },
    {
      name: "Matter brief",
      line: "Keeps a short brief of the matter in its notes folder: parties, facts, forum, positions taken and open questions.",
      tag: "Every matter",
    },
    {
      name: "Word documents",
      line: "Opens and edits Word files in the matter, with each change shown as a tracked change you can accept or reject.",
      tag: "Every matter",
    },
    { name: "Notice drafting", line: "From a folder on this computer", tag: "Every matter" },
    { name: "Lease review", line: "From a folder on this computer", tag: "Stopped" },
    { name: "Old briefs", line: "From a folder on this computer", tag: "Turned off" },
  ]);
  const heads = [...h!.host.querySelectorAll(".lib-group-head")].map((e) => e.textContent);
  expect(heads).toEqual(["Comes with Clauseo", "Added by you"]);
  // L2: an added skill has a switch, a built-in skill none (library-switches.test.tsx)
  expect(
    h!.one("library")?.querySelector('[data-id="notice-drafting"] + [role="switch"]'),
  ).not.toBeNull();
  expect(
    h!.one("library")?.querySelector('[data-id="legal-drafting"] + [role="switch"]'),
  ).toBeNull();
});

test("with no skill added, Added by you says so", async () => {
  await mount([]);
  await open();
  expect(h!.host.querySelector(".lib-empty")?.textContent).toBe("You haven't added a skill yet.");
});

test("the side panel explains skills, naming the assistant, until a row is picked; then the item, read-only", async () => {
  await mount();
  await open();
  const explain = text("library-explainer")!;
  expect(explain).toContain("What a skill is");
  expect(explain).toContain("When a question calls for it, Claude reads the skill and follows it.");
  await h!.click(row("Legal drafting"));
  expect(h!.one("library-explainer")).toBeNull();
  const detail = h!.one("library-detail")!;
  expect(detail.querySelector(".lib-detail-name")?.textContent).toBe("Legal drafting");
  expect(detail.querySelector(".lib-detail-head p")?.textContent).toBe("Comes with Clauseo");
  expect(detail.querySelector(".lib-detail-label")?.textContent).toBe("Where Claude uses it");
  expect(text("library-detail-where")).toBe("Every matter");
  expect(detail.querySelector("button")).toBeNull();
  await h!.click(row("Lease review"));
  expect(h!.one("library-detail")?.querySelector(".lib-trouble")?.textContent).toBe(
    "Its files changed after you added it, so Clauseo stopped using it.",
  );
  // a second press on the picked row goes back to the explainer
  await h!.click(row("Lease review"));
  expect(h!.one("library-explainer")).not.toBeNull();
});

test("Clauseo Case Law not added: the agreed words, and its button opens Settings at Connectors", async () => {
  await mount();
  await play({ connectors_changed: caseLaw("not_added") });
  await open();
  await tab("Connectors");
  const card = h!.one("case-law-card")!;
  expect(card.getAttribute("data-shown")).toBe("offer");
  expect(card.querySelector(".lib-feature-name")?.textContent).toBe("Clauseo Case Law");
  expect(card.querySelector(".lib-powered")?.textContent).toBe("Powered by Indian Kanoon");
  expect(card.querySelector(".lib-feature-line")?.textContent).toBe(GIVES);
  expect(card.querySelector(".lib-feature-meta")?.textContent).toBe(
    "From ₹236.Prepaid, including GST. No subscription.",
  );
  expect(card.querySelector("img")).toBeNull();
  const add = card.querySelector(".lib-feature-add")!;
  expect(add.textContent).toBe("Add Clauseo Case Law");
  await h!.click(add);
  expect(h!.transport.calls).toContainEqual({ call: "open_settings", section: "connectors" });
  // picked, the panel says what it covers and costs, and offers Add again
  await h!.click(card.querySelector(".lib-feature-hit"));
  const detail = text("library-detail")!;
  expect(detail).toContain("What it covers");
  expect(detail).toContain("For any judgment, Claude can see which later judgments cite it");
  expect(detail).toContain("plus 1 for each 4,000 tokens Claude reads");
  expect(detail).toContain("Add Clauseo Case Law");
});

test("Clauseo Case Law added: its credits and where it is on", async () => {
  await mount();
  await play({
    connectors_changed: caseLaw({ signed_in: { low: false, balance: { credits: 1_240 } } }),
  });
  await open();
  await tab("Connectors");
  const card = h!.one("case-law-card")!;
  expect(card.getAttribute("data-shown")).toBe("added");
  expect(card.querySelector(".lib-feature-meta")?.textContent).toBe(
    "1,240 credits leftEvery matter",
  );
  expect(card.querySelector("button.lib-feature-add")).toBeNull();
  await h!.click(card.querySelector(".lib-feature-hit"));
  expect(text("library-detail")).toContain("1,240 credits left");
  // no count of searches anywhere: what credits buy depends on the question
  expect(h!.one("library")?.textContent).not.toMatch(/searches/);
});

test("no answer from Case Law: unreachable, and Retry reads the list again, which checks again", async () => {
  await mount();
  await play({ connectors_changed: caseLaw(null) });
  await open();
  await tab("Connectors");
  const card = h!.one("case-law-card")!;
  expect(card.getAttribute("data-shown")).toBe("unreachable");
  expect(card.querySelector(".lib-feature-name")?.textContent).toBe("Clauseo Case Law");
  expect(card.querySelector(".lib-feature-line")?.textContent).toBe(GIVES);
  expect(text("case-law-trouble")).toBe("Can't reach it right now");
  const retry = card.querySelector(".lib-feature-add")!;
  expect(retry.textContent).toBe("Retry");
  const before = reads;
  await h!.click(retry);
  await h!.settle();
  expect(reads).toBe(before + 1);
  expect(h!.transport.calls).not.toContainEqual({ call: "open_settings", section: "connectors" });
  await h!.click(card.querySelector(".lib-feature-hit"));
  expect(h!.one("library-detail")?.querySelector(".lib-trouble p")?.textContent).toBe(
    "Clauseo can't reach Clauseo Case Law right now, so Claude can't search judgments.",
  );
});

test("Clauseo Case Law turned off by the firm says Not allowed, with no button", async () => {
  await mount();
  await play({ connectors_changed: caseLaw(null, false) });
  await open();
  await tab("Connectors");
  expect(text("case-law-trouble")).toBe("Not allowed");
  expect(h!.one("case-law-card")?.querySelector(".lib-feature-add")).toBeNull();
});

test("Connectors: Case Law first, then the Web browser, then your own connectors, MCP servers said", async () => {
  await mount();
  await play({ mcp_tools_changed: TOOLS });
  await open();
  await tab("Connectors");
  const list = h!.host.querySelector(".lib-list")!;
  expect(list.firstElementChild?.getAttribute("data-testid")).toBe("case-law-card");
  const heads = [...list.querySelectorAll(".lib-group-head")].map((e) => e.textContent);
  expect(heads).toEqual(["Comes with Clauseo", "Your own connectorsMCP servers"]);
  expect(rows()).toEqual([
    {
      name: "Web browser",
      line: "Opens web pages that need a real browser, such as court websites with forms to fill.",
      tag: "Every matter",
    },
    { name: "court-diary", line: "MCP server, a program on this computer", tag: "Every matter" },
    { name: "firm-precedents", line: "MCP server at precedents.raoandco.in", tag: "Key missing" },
  ]);
  expect(text("library-explainer")).toContain("Your own connectors are MCP servers");
  await h!.click(row("court-diary"));
  const facts = h!.one("library-detail")?.querySelector(".lib-facts")?.textContent;
  expect(facts).toBe("Commandnpx -y court-diary-mcpSaved valuesDIARY_TOKEN");
});

test("opening a matter from the rail closes the place", async () => {
  await mount();
  await open();
  const matter = h!.host.querySelector<HTMLElement>(".room-matters .room-matter-row");
  await h!.click(matter);
  expect(h!.one("library")).toBeNull();
  expect(text("matter")).toBe("Sharma v. State of Delhi");
});

test("before the connectors' state arrives, Case Law waits: no offer, nothing to press", async () => {
  await mount();
  await open();
  await tab("Connectors");
  const card = h!.one("case-law-card")!;
  expect(card.getAttribute("data-shown")).toBe("waiting");
  expect(card.getAttribute("aria-busy")).toBe("true");
  expect(card.querySelector(".lib-feature-add")).toBeNull();
  expect(card.textContent).not.toContain("From ₹236");
  await h!.click(card.querySelector(".lib-feature-hit"));
  expect(text("library-detail")).not.toContain("Add Clauseo Case Law");
  // a lawyer who holds credits, once the state arrives, sees them, never the offer
  await play({
    connectors_changed: caseLaw({ signed_in: { low: false, balance: { credits: 400 } } }),
  });
  expect(h!.one("case-law-card")?.getAttribute("data-shown")).toBe("added");
});

test("connectors turned off by the lawyer: Turned off, and how to turn them on", async () => {
  await mount();
  await play({ connectors_changed: caseLaw("not_added", false, false) });
  await open();
  await tab("Connectors");
  const card = h!.one("case-law-card")!;
  expect(card.getAttribute("data-shown")).toBe("off");
  expect(text("case-law-trouble")).toBe("Turned off");
  const button = card.querySelector(".lib-feature-add")!;
  expect(button.textContent).toBe("Open Settings");
  await h!.click(button);
  expect(h!.transport.calls).toContainEqual({ call: "open_settings", section: "connectors" });
  await h!.click(card.querySelector(".lib-feature-hit"));
  const detail = text("library-detail")!;
  expect(detail).toContain(
    "Connectors are turned off. You can turn them on in Settings, under Connectors.",
  );
  expect(detail).not.toContain("Add Clauseo Case Law");
});

test.each([
  ["signed_out", "ended", "Signed out", "Open Settings"],
  ["key_revoked", "ended", "Signed out", "Open Settings"],
] as const)(
  "Case Law whose sign-in ended (%s): says so and opens Settings, never the offer",
  async (status, shown, tag, button) => {
    await mount();
    await play({ connectors_changed: caseLaw(status) });
    await open();
    await tab("Connectors");
    const card = h!.one("case-law-card")!;
    expect(card.getAttribute("data-shown")).toBe(shown);
    expect(text("case-law-trouble")).toBe(tag);
    expect(card.querySelector(".lib-feature-add")?.textContent).toBe(button);
    expect(card.textContent).not.toContain("From ₹236");
    await h!.click(card.querySelector(".lib-feature-hit"));
    const detail = text("library-detail")!;
    expect(detail).toContain("Sign in again in Settings, under Connectors.");
    expect(detail).not.toContain("Add Clauseo Case Law");
  },
);

test("Case Law signing in: the waiting card, nothing to press", async () => {
  await mount();
  await play({ connectors_changed: caseLaw("signing_in") });
  await open();
  await tab("Connectors");
  const card = h!.one("case-law-card")!;
  expect(card.getAttribute("data-shown")).toBe("waiting");
  expect(card.querySelector(".lib-feature-add")).toBeNull();
});

test("Case Law whose last sign-in failed: the host lists it needs_setup, so it is offered again", async () => {
  await mount();
  await play({ connectors_changed: caseLaw({ error: { message: "refused" } }) });
  await open();
  await tab("Connectors");
  expect(h!.one("case-law-card")?.getAttribute("data-shown")).toBe("offer");
});

test("Case Law not offered: Not available yet., no button, no Retry, and not in the rail's count", async () => {
  await mount();
  await play({ mcp_tools_changed: TOOLS });
  await play({ connectors_changed: caseLaw(null) }, "not_offered");
  expect(h!.one("library-entry")?.textContent).toBe(
    "Skills and connectors7 skills, 2 connectorsNew",
  );
  await open();
  await tab("Connectors");
  const card = h!.one("case-law-card")!;
  expect(card.getAttribute("data-shown")).toBe("unavailable");
  expect(text("case-law-trouble")).toBe("Not available yet.");
  expect(card.querySelector("button.lib-feature-add")).toBeNull();
  await h!.click(card.querySelector(".lib-feature-hit"));
  const detail = h!.one("library-detail")!;
  expect(detail.querySelector(".lib-trouble")?.textContent).toBe("Not available yet.");
  expect(detail.textContent).not.toContain("Retry");
  expect(detail.textContent).not.toContain("Add Clauseo Case Law");
});

test("Case Law not yet checked (the host lists it nowhere yet): needs_setup, the offer, never Can't reach", async () => {
  await mount();
  await open();
  await tab("Connectors");
  // listed by the place before the host lists it, waiting for the connectors' state
  expect(h!.one("case-law-card")?.getAttribute("data-shown")).toBe("waiting");
  await play({ connectors_changed: caseLaw("not_added") }, null);
  const card = h!.one("case-law-card")!;
  expect(card.getAttribute("data-shown")).toBe("offer");
  expect(h!.one("library")?.textContent).not.toContain("Can't reach it right now");
});

test("Clauseo Case Law not added, with no price listed: the terms alone, no count", async () => {
  await mount();
  await play({
    connectors_changed: {
      available: true,
      locked: false,
      connectors: [{ id: "case-law", name: "Indian Kanoon", packs: [], status: "not_added" }],
    },
  });
  await open();
  await tab("Connectors");
  const card = h!.one("case-law-card")!;
  expect(card.getAttribute("data-shown")).toBe("offer");
  expect(card.querySelector(".lib-feature-meta")?.textContent).toBe(
    "Prepaid credits. No subscription.",
  );
  expect(card.textContent).not.toMatch(/searches/);
});

// stage E (Mac keychain prompting off): while the window's one permissions read lists the keychain row as would_ask, a
// signed-out Case Law and a tool's missing key may be only unreadable, so they point at the row's Allow
const KEYS: PermissionRow = {
  id: "keychain",
  state: "would_ask",
  folder: null,
  settings_path: null,
  note: null,
};
const POINTER = "Clauseo needs your Mac's permission to read its saved keys.";

/** The window's read (the band's, as it would make it) listing `rows`; each ask answers granted. */
async function heard(rows: PermissionRow[]) {
  const ask = vi.fn(async () => ({ ...KEYS, state: "granted" as const }));
  providePermissions(h!.transport, { list: async () => rows, ask });
  await act(async () => void (await permissionsHostOf(h!.transport).list()));
  await h!.settle();
  return ask;
}

test("keychain would_ask: Case Law signed out points at the keychain's Allow, never Signed out or Sign in again", async () => {
  await mount();
  await play({ connectors_changed: caseLaw("signed_out") });
  const ask = await heard([KEYS]);
  await open();
  await tab("Connectors");
  const card = h!.one("case-law-card")!;
  expect(text("case-law-trouble")).toBe("Needs your Mac's permission");
  expect(card.querySelector(".lib-feature-add")?.textContent).toBe("Allow");
  await h!.click(card.querySelector(".lib-feature-hit"));
  const detail = text("library-detail")!;
  expect(detail).toContain(POINTER);
  expect(detail).not.toContain("Sign in again in Settings, under Connectors.");
  await h!.click(h!.one("library-detail")!.querySelector('[data-testid="keychain-allow"]'));
  expect(ask).toHaveBeenCalledWith("keychain");
  expect(settings).not.toHaveBeenCalled();
});

test("keychain would_ask: a tool's missing key points at the keychain's Allow, never Key missing", async () => {
  await mount();
  await play({ mcp_tools_changed: TOOLS });
  const ask = await heard([KEYS]);
  await open();
  await tab("Connectors");
  expect(row("firm-precedents")?.querySelector(".lib-row-tag")?.textContent).toBe(
    "Needs your Mac's permission",
  );
  await h!.click(row("firm-precedents"));
  const detail = text("library-detail")!;
  expect(detail).toContain(POINTER);
  expect(detail).not.toContain("Its saved key is missing");
  await h!.click(h!.one("library-detail")!.querySelector('[data-testid="keychain-allow"]'));
  expect(ask).toHaveBeenCalledWith("keychain");
});

test.each([
  ["granted", [{ ...KEYS, state: "granted" as const }]],
  ["not listed", []],
] as const)("keychain %s: Case Law and the tool keep today's words", async (_, rows) => {
  await mount();
  await play({ mcp_tools_changed: TOOLS });
  await play({ connectors_changed: caseLaw("signed_out") });
  await heard([...rows]);
  await open();
  await tab("Connectors");
  expect(text("case-law-trouble")).toBe("Signed out");
  expect(row("firm-precedents")?.querySelector(".lib-row-tag")?.textContent).toBe("Key missing");
  await h!.click(row("firm-precedents"));
  expect(text("library-detail")).toContain("Its saved key is missing");
  expect(h!.host.textContent).not.toContain(POINTER);
});

// #1348: while Case Law's saved sign-in can't be read, the host says Error { kind: sign_in_failed } and leaves it out of
// list_addons; the card points at the keychain row by the row's own state, and granted keeps today's offer
const FAILED: ConnectorStatus = { error: { message: "keychain refused", kind: "sign_in_failed" } };

test("keychain would_ask: a failed Case Law sign-in, left out of the list, points at the keychain's Allow", async () => {
  await mount();
  await play({ connectors_changed: caseLaw(FAILED) }, null);
  const ask = await heard([KEYS]);
  await open();
  await tab("Connectors");
  const card = h!.one("case-law-card")!;
  expect(text("case-law-trouble")).toBe("Needs your Mac's permission");
  expect(card.querySelector(".lib-feature-add")?.textContent).toBe("Allow");
  await h!.click(card.querySelector(".lib-feature-hit"));
  expect(text("library-detail")).toContain(POINTER);
  await h!.click(h!.one("library-detail")!.querySelector('[data-testid="keychain-allow"]'));
  expect(ask).toHaveBeenCalledWith("keychain");
});

test("keychain granted: a failed Case Law sign-in shows as today, the offer", async () => {
  await mount();
  await play({ connectors_changed: caseLaw(FAILED) }, null);
  await heard([{ ...KEYS, state: "granted" }]);
  await open();
  await tab("Connectors");
  const card = h!.one("case-law-card")!;
  expect(card.querySelector(".lib-feature-add")?.textContent).toBe("Add Clauseo Case Law");
  expect(h!.one("case-law-trouble")).toBeNull();
  expect(h!.host.textContent).not.toContain(POINTER);
});
