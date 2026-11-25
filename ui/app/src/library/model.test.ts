// What the place says about an item, from its list_addons fields (model.ts), the list as read from
// list_addons (addons-list.ts), Case Law's state from its status (case-law-model.ts), and the
// assistant's name (assistant.ts).
import { expect, test } from "vitest";
import type { AddonView } from "./addons.ts";
import { act } from "react";
import { FakeTransport } from "../transport/fake-transport.ts";
import type { AddonStatus, McpServerView, QueryResult } from "../transport/bindings.ts";
import { addonsList, listed } from "./addons-list.ts";
import { caseLawFacts, type CaseLawFacts } from "./addons-source.ts";
import { caseLawShown } from "./case-law-model.ts";
import { assistantName } from "./assistant.ts";
import { fromLine, haveCount, nameOf, rowLine, scopeTag } from "./model.ts";

const NAMES = new Map([
  ["sharma", "Sharma v. State of Delhi"],
  ["mehta", "Mehta Textiles arbitration"],
]);

const item = (patch: Partial<AddonView>): AddonView => ({
  kind: "skill",
  id: "pdf",
  name: "PDF",
  source: "added",
  description: "Reads, fills and merges PDF files.",
  scope: "all",
  on: true,
  status: "ready",
  can_remove: true,
  locked: false,
  ...patch,
});

test("where it is on: every matter (but some), only the matters switched on, or turned off", () => {
  expect(scopeTag("en", item({}), NAMES)).toBe("Every matter");
  expect(scopeTag("en", item({ on: false }), NAMES)).toBe("Turned off");
  expect(scopeTag("en", item({ scope: { matter: "sharma" }, switched: ["sharma"] }), NAMES)).toBe(
    "Only Sharma v. State of Delhi",
  );
  expect(
    scopeTag("en", item({ scope: { matter: "sharma" }, switched: ["sharma", "mehta"] }), NAMES),
  ).toBe("Only 2 matters");
  expect(scopeTag("en", item({ switched: ["mehta"] }), NAMES)).toBe("Every matter but 1");
  // on in no matter: scope matter with none switched on, never "Only <matter>"
  expect(scopeTag("en", item({ scope: { matter: "sharma" } }), NAMES)).toBe(
    "Not used in any matter",
  );
  // a matter the room no longer knows is not counted; under the firm's lock, every matter
  expect(scopeTag("en", item({ switched: ["gone"] }), NAMES)).toBe("Every matter");
  expect(scopeTag("en", item({ switched: ["mehta"], locked: true }), NAMES)).toBe("Every matter");
});

test("where it came from, MCP server said for the lawyer's own connectors", () => {
  const from = (kind: NonNullable<AddonView["from"]>["kind"], text: string) =>
    fromLine("en", item({ from: { kind, text } }));
  expect(from("skills_sh", "anthropics/skills/pdf")).toBe("From skills.sh, anthropics/skills/pdf");
  expect(from("github", "github.com/lexdesk/skills")).toBe("From GitHub, lexdesk/skills");
  expect(from("url", "https://example.org/skill.zip")).toBe("From example.org");
  expect(from("zip", "~/Downloads/a.zip")).toBe("From a zip file on this computer");
  expect(from("program", "npx")).toBe("MCP server, a program on this computer");
  expect(from("address", "https://mcp.gazettewatch.in")).toBe("MCP server at mcp.gazettewatch.in");
  expect(from("import", "Claude Desktop")).toBe("MCP server, brought over from Claude Desktop");
  expect(fromLine("en", item({ source: "built_in" }))).toBe("Comes with Clauseo");
});

test("a built-in takes the app's own name and line; an added item without a line shows where it came from", () => {
  const drafting = item({ id: "legal-drafting", name: "legal-drafting", source: "built_in" });
  expect(nameOf("en", drafting)).toBe("Legal drafting");
  expect(rowLine("en", drafting)).toMatch(/^Drafts and revises notices/);
  // an added skill that happens to share a built-in's id keeps its own name
  expect(nameOf("en", item({ id: "legal-drafting", name: "My drafting" }))).toBe("My drafting");
  const tool = item({ kind: "tool", description: "", from: { kind: "program", text: "npx" } });
  expect(rowLine("en", tool)).toBe("MCP server, a program on this computer");
});

test("the assistant: the one plan signed in, the room's plan before any sign-in, and both said plainly", () => {
  expect(assistantName(new Set(["codex"]), "claude")).toBe("ChatGPT");
  expect(assistantName(new Set(["claude"]), "codex")).toBe("Claude");
  expect(assistantName(null, "codex")).toBe("ChatGPT");
  expect(assistantName(null, undefined)).toBe("Claude");
  expect(assistantName(new Set(["claude", "codex"]), "claude")).toBeNull();
});

const builtIn = (kind: AddonView["kind"], id: string, status: AddonStatus = "ready") =>
  item({ kind, id, name: id, source: "built_in", description: "", can_remove: false, status });

const HOST_ITEMS: AddonView[] = [
  builtIn("skill", "legal-drafting"),
  builtIn("skill", "legal-research"),
  builtIn("skill", "matter-brief"),
  builtIn("skill", "stetdoc"),
  builtIn("tool", "case-law", "needs_setup"),
  builtIn("tool", "browser"),
];

const SERVER: McpServerView = {
  name: "court-diary",
  kind: "local",
  command: "npx",
  args: ["-y", "court-diary-mcp"],
  env: [{ name: "DIARY_TOKEN", saved: true }],
  headers: [],
  enabled: true,
};

test("the list: Case Law not yet listed by the host reads needs_setup, and a tool takes its server by name", () => {
  const noCaseLaw = HOST_ITEMS.filter((a) => a.id !== "case-law");
  const rows = listed(noCaseLaw, []);
  expect(rows.map((a) => a.id)).toEqual([
    "legal-drafting",
    "legal-research",
    "matter-brief",
    "stetdoc",
    "case-law",
    "browser",
  ]);
  // not yet checked: needs_setup, never unreachable, so no "can't reach" flash at start-up
  expect(rows.find((a) => a.id === "case-law")?.status).toBe("needs_setup");
  const tool = item({ kind: "tool", id: "court-diary", name: "court-diary", description: "" });
  const joined = listed([...HOST_ITEMS, tool], [SERVER]).find((a) => a.id === "court-diary");
  expect(joined?.server).toEqual(SERVER);
  const own = { ...SERVER, args: ["court-diary-mcp"] };
  const kept = listed([{ ...tool, server: own }], [SERVER]).find((a) => a.id === "court-diary");
  expect(kept?.server).toBe(own);
  expect(listed([...HOST_ITEMS, tool], []).find((a) => a.id === "court-diary")?.server).toBe(
    undefined,
  );
});

const HEARD: CaseLawFacts = {
  heard: true,
  off: null,
  pack: null,
  credits: null,
  out: null,
  signIn: null,
};

test("Case Law's state from each status the host gives", () => {
  const shown = (status: AddonStatus, facts: Partial<CaseLawFacts> = {}) =>
    caseLawShown(builtIn("tool", "case-law", status), { ...HEARD, ...facts });
  expect(shown("ready", { credits: 1_240 })).toBe("added");
  expect(shown("needs_credit", { credits: 0 })).toBe("added");
  expect(shown("needs_setup")).toBe("offer");
  // out of credit by the connectors (no figure) while the last list read still said needs_setup
  expect(shown("needs_setup", { out: { payUrl: null } })).toBe("added");
  // the connectors' state not heard yet: the offer waits
  expect(shown("needs_setup", { heard: false })).toBe("waiting");
  expect(shown("signing_in")).toBe("waiting");
  expect(shown("signed_out")).toBe("ended");
  expect(shown("unreachable")).toBe("unreachable");
  expect(shown("not_offered")).toBe("unavailable");
  expect(shown("blocked")).toBe("trouble");
  expect(shown("not_offered", { off: "lawyer" })).toBe("off");
  expect(shown("needs_setup", { off: "firm" })).toBe("trouble");
  // a lawyer who holds credits is never offered them, whatever the list last said
  expect(shown("needs_setup", { credits: 200 })).toBe("added");
});

test("Case Law's facts and the rail's count: what the lawyer has", () => {
  const off = { available: false, locked: false, connectors: [] };
  expect(caseLawFacts(off, false)).toEqual({
    heard: false,
    off: null,
    pack: null,
    credits: null,
    out: null,
    signIn: null,
  });
  expect(caseLawFacts(off, true).off).toBe("lawyer");
  expect(caseLawFacts({ ...off, locked: true }, true).off).toBe("firm");
  const signedIn = {
    available: true,
    locked: false,
    connectors: [
      {
        id: "case-law",
        name: "Indian Kanoon",
        packs: [
          { pack: "big", credits: 10_000, paise: 99_000 },
          { pack: "small", credits: 2_000, paise: 23_600 },
        ],
        status: { signed_in: { low: false, balance: { credits: 1_240 } } },
      },
    ],
  };
  const facts = caseLawFacts(signedIn, true);
  expect(facts).toEqual({
    heard: true,
    off: null,
    pack: { paise: 23_600, credits: 2_000 },
    credits: 1_240,
    out: null,
    signIn: null,
  });
  const withCaseLaw = (status: AddonStatus) =>
    HOST_ITEMS.map((a) => (a.id === "case-law" ? { ...a, status } : a));
  expect(haveCount(withCaseLaw("ready"), facts)).toEqual({ skills: 4, connectors: 1 });
  expect(haveCount(withCaseLaw("needs_setup"), HEARD)).toEqual({ skills: 4, connectors: 0 });
  // not offered is left out of the count
  expect(haveCount(withCaseLaw("not_offered"), HEARD)).toEqual({ skills: 4, connectors: 0 });
});

/** A transport whose list_addons answers `answer()` and counts the reads, with an app feed. */
function listHost(answer: () => AddonView[]) {
  let reads = 0;
  const transport = new FakeTransport({
    next: { parts: ["app"], query: () => ({ threads: [] }) },
    query: (query) => {
      if (typeof query === "object" && "list_addons" in query) {
        reads += 1;
        return { addons: { items: answer() } } as QueryResult;
      }
      throw new Error("not scripted");
    },
  });
  return { transport, reads: () => reads };
}

const tick = () => act(async () => new Promise((resolve) => setTimeout(resolve, 0)));

test("the reader asks list_addons, again on addons_changed and on read(), and is complete once the connectors' state is heard", async () => {
  let items = HOST_ITEMS;
  const host = listHost(() => items);
  const list = addonsList(host.transport);
  await tick();
  expect(host.reads()).toBe(1);
  expect(list.state().items?.map((a) => a.id)).toContain("case-law");
  expect(list.state().complete).toBe(false);
  host.transport.appEvent({
    connectors_changed: { available: true, locked: false, connectors: [] },
  });
  expect(list.state().complete).toBe(true);
  items = HOST_ITEMS.map((a) => (a.id === "case-law" ? { ...a, status: "not_offered" } : a));
  host.transport.appEvent({ addons_changed: {} });
  await tick();
  expect(host.reads()).toBe(2);
  expect(list.state().items?.find((a) => a.id === "case-law")?.status).toBe("not_offered");
  list.read();
  await tick();
  expect(host.reads()).toBe(3);
  host.transport.appEvent({
    mcp_tools_changed: { control: "lawyer", servers: [SERVER], importable: [] },
  });
  expect(list.state().items).not.toBeNull();
});

test("with no app feed, the list is complete once list_addons answers: the rail's count shows", async () => {
  const transport = new FakeTransport({
    query: (query) => {
      if (typeof query === "object" && "list_addons" in query)
        return { addons: { items: HOST_ITEMS } } as QueryResult;
      throw new Error("not scripted");
    },
  });
  const list = addonsList(transport);
  expect(list.state().complete).toBe(false);
  await tick();
  expect(list.state()).toMatchObject({ complete: true });
  expect(list.state().items?.length).toBe(6);
});
