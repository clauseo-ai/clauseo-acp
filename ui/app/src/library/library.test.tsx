// @vitest-environment happy-dom Skills and connectors in the real room over FakeTransport (library
// L1, read-only): the rail's row opens the place in the centre with the wall hidden; Skills lists
// the built-ins and the added skill packs with where each is on; Connectors leads with Clauseo Case
// Law in each of its states, then the Web browser, then the lawyer's own MCP servers; the side
// panel explains the part until a row is picked, then shows that item; the words name the
// assistant. Opening a matter closes the place.
import { act } from "react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { FakeTransport } from "../transport/fake-transport.ts";
import type {
  ConnectorStatus,
  ConnectorsState,
  McpToolsView,
  PackStatus,
} from "../transport/protocol.ts";
import { mountRoom, type Harness } from "../room/thread/harness.ts";
import { SEEN_KEY } from "./seen.ts";

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

async function mount(packs: PackStatus[] = PACKS) {
  settings.mockReset();
  h = await mountRoom({
    next: { parts: ["app"], query: () => ({ threads: [] }) },
    query: (query) => {
      if (query === "list_skill_packs") return { skill_packs: packs };
      throw new Error(`not scripted: ${JSON.stringify(query)}`);
    },
    openSettings: settings,
  });
  await h.settle();
  return h;
}

const play = async (event: Parameters<FakeTransport["appEvent"]>[0]) => {
  await act(async () => h!.transport.appEvent(event));
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
  // nothing to switch, add or remove yet (L2 to L4)
  expect(h!.one("library")?.querySelector('[role="switch"]')).toBeNull();
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
  await h!.click(row("Legal drafting")?.querySelector("button"));
  expect(h!.one("library-explainer")).toBeNull();
  const detail = h!.one("library-detail")!;
  expect(detail.querySelector(".lib-detail-name")?.textContent).toBe("Legal drafting");
  expect(detail.querySelector(".lib-detail-head p")?.textContent).toBe("Comes with Clauseo");
  expect(detail.querySelector(".lib-detail-label")?.textContent).toBe("Where Claude uses it");
  expect(text("library-detail-where")).toBe("Every matter");
  expect(detail.querySelector("button")).toBeNull();
  await h!.click(row("Lease review")?.querySelector("button"));
  expect(h!.one("library-detail")?.querySelector(".lib-trouble")?.textContent).toBe(
    "Its files changed after you added it, so Clauseo stopped using it.",
  );
  // a second press on the picked row goes back to the explainer
  await h!.click(row("Lease review")?.querySelector("button"));
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
    "From ₹236, about 100 searchesPrepaid, including GST. No subscription.",
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
    "1,240 credits leftAbout 62 searchesEvery matter",
  );
  expect(card.querySelector("button.lib-feature-add")).toBeNull();
  await h!.click(card.querySelector(".lib-feature-hit"));
  expect(text("library-detail")).toContain("1,240 credits left · About 62 searches");
});

test("the host lists no connector (its liveness check failed): Case Law is unreachable, and its button opens Settings to check", async () => {
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
  expect(retry.textContent).toBe("Open Settings to check");
  await h!.click(retry);
  expect(h!.transport.calls).toContainEqual({ call: "open_settings", section: "connectors" });
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
  await h!.click(row("court-diary")?.querySelector("button"));
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
