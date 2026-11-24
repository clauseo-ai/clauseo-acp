// What the place says about an item, from its list_addons fields (model.ts), the interim list from
// today's reads (addons-interim.ts), and the assistant's name (assistant.ts).
import { expect, test } from "vitest";
import type { AddonView } from "./addons.ts";
import {
  caseLawStatus,
  interimItems,
  interimList,
  packItem,
  serverItem,
} from "./addons-interim.ts";
import { FakeTransport } from "../transport/fake-transport.ts";
import { caseLawFacts } from "./addons-source.ts";
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

test("where it is on: every matter, only the matters switched on, or turned off", () => {
  expect(scopeTag("en", item({}), NAMES)).toBe("Every matter");
  expect(scopeTag("en", item({ on: false }), NAMES)).toBe("Turned off");
  expect(scopeTag("en", item({ scope: "matter", switched: ["sharma"] }), NAMES)).toBe(
    "Only Sharma v. State of Delhi",
  );
  expect(scopeTag("en", item({ scope: "matter", switched: ["sharma", "mehta"] }), NAMES)).toBe(
    "Only Sharma v. State of Delhi and Mehta Textiles arbitration",
  );
  // without the host's list of matters, nothing rather than a guess
  expect(scopeTag("en", item({ scope: "matter" }), NAMES)).toBe("");
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

test("the interim list: packs, servers and Case Law's status from today's reads", () => {
  expect(
    packItem({ id: "x", state: "on", source_url: "https://github.com/lexdesk/skills" }),
  ).toMatchObject({
    from: { kind: "github", text: "lexdesk/skills" },
    status: "ready",
    can_remove: false,
  });
  expect(packItem({ id: "y", state: "not_admitted", instruction_only: false })).toMatchObject({
    status: "blocked",
    has_scripts: true,
  });
  const server = serverItem(
    {
      name: "s",
      kind: "remote",
      url: "https://a.in/mcp",
      args: [],
      env: [],
      headers: [],
      enabled: false,
    },
    true,
  );
  expect(server).toMatchObject({
    from: { kind: "address" },
    on: false,
    locked: true,
    can_remove: false,
  });
  const off = { available: false, locked: false, connectors: [] };
  expect(caseLawStatus(off, false)).toBe("needs_setup");
  expect(caseLawStatus({ ...off, available: true }, true)).toBe("unreachable");
  expect(caseLawStatus({ ...off, locked: true }, true)).toBe("blocked");
  const ids = interimItems([], null, off, false).map((a) => a.id);
  expect(ids).toEqual([
    "legal-drafting",
    "legal-research",
    "matter-brief",
    "stetdoc",
    "case-law",
    "browser",
  ]);
});

test("Case Law's facts and the rail's count: what the lawyer has", () => {
  const off = { available: false, locked: false, connectors: [] };
  expect(caseLawFacts(off, false)).toEqual({
    heard: false,
    off: null,
    pack: null,
    credits: null,
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
    signIn: null,
  });
  const items = interimItems([], null, signedIn, true);
  expect(haveCount(items, facts)).toEqual({ skills: 4, connectors: 1 });
  expect(haveCount(interimItems([], null, off, true), caseLawFacts(off, true))).toEqual({
    skills: 4,
    connectors: 0,
  });
});

test("with no app feed, the list is complete once the packs are read: the rail's count shows", async () => {
  const transport = new FakeTransport({
    query: (query) => {
      if (query === "list_skill_packs") return { skill_packs: [] };
      throw new Error("not scripted");
    },
  });
  const list = interimList(transport);
  expect(list.state().complete).toBe(false);
  await new Promise((resolve) => setTimeout(resolve, 0));
  expect(list.state()).toMatchObject({ complete: true });
  expect(list.state().items?.length).toBe(6);
});
