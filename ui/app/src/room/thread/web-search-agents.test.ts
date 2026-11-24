// ChatGPT's web search in the online words Claude's has. codex-acp 2.2.2 (`WebSearchReporter`) sends it with the kind
// search, raw input `{query, action}` (live, also `type: "webSearch"` and its id), titled "Web search", then patched to
// "Web search: <query>" (or "Open page: <url>"); Claude's WebSearch comes with the kind fetch and `{query}`. Both read
// "Searching online for '…'." and the like, count as looked online and draw the globe; a search of the papers (a
// `pattern`, Claude's Grep and Glob) under either agent keeps its own words.
import { stepLive } from "@clauseo/components/lib/words-steps-live.ts";
import { steps } from "@clauseo/components/lib/words-steps.ts";
import { expect, test } from "vitest";
import { toolCall } from "../../testing/stream-script.ts";
import type { ToolCallEntry, ToolCallStatus, ToolKind } from "../../transport/protocol.ts";
import { doingOf, webSearchOf } from "./step-names.ts";
import { doneOf, drawnKindOf, iconsOf, summaryOf } from "./steps.ts";

const entry = (
  title: string,
  kind: ToolKind,
  status: ToolCallStatus,
  raw: object | null,
): ToolCallEntry => ({
  ...toolCall(7, 290, status).tool_call!,
  title,
  kind,
  ...(raw === null ? {} : { raw_input: { inline: JSON.stringify(raw) } }),
});

const Q = "section 138 NI Act judgment";
/** ChatGPT's search, done or not, in the AIR shape and in the live shape that names the item. */
const chatgpt = (status: ToolCallStatus, live = false, query = Q) =>
  entry(`Web search: ${query}`, "search", status, {
    ...(live ? { type: "webSearch", id: "ws_1" } : {}),
    query,
    action: { type: "search", query, queries: null },
  });
const claude = (status: ToolCallStatus, query = Q) =>
  entry(`Search "${query}"`, "fetch", status, { query });

test.each([
  ["ChatGPT (AIR shape)", (s: ToolCallStatus) => chatgpt(s)],
  ["ChatGPT (live shape)", (s: ToolCallStatus) => chatgpt(s, true)],
  ["Claude", (s: ToolCallStatus) => claude(s)],
] as const)("a web search under %s reads online: running, done and failed", (_agent, make) => {
  expect(doingOf("en", make("in_progress"))).toBe(
    stepLive("en", "Searching online for '{query}'.", { query: Q }),
  );
  expect(doneOf("en", make("completed"))).toBe(
    steps("en", "Searched online for '{query}'.", { query: Q }),
  );
  expect(doneOf("en", make("failed"))).toBe(
    steps("en", "Tried to search online for '{query}'.", { query: Q }),
  );
  expect(summaryOf("en", [make("completed")])).toBe("Looked online.");
  expect(drawnKindOf(make("completed"))).toBe("fetch");
  expect(iconsOf([make("completed")])).toEqual(["fetch"]);
});

test("ChatGPT's search before its query comes says it is looking online, never 'Web search.' nor the papers", () => {
  const start = entry("Web search", "search", "in_progress", { query: "", action: null });
  expect(webSearchOf(start)).toBe(true);
  expect(doingOf("en", start)).toBe(stepLive("en", "Looking online."));
  // a search whose input never came: its title alone
  const bare = entry("Web search", "search", "completed", null);
  expect(doneOf("en", bare)).toBe(steps("en", "Looked online."));
  expect(doneOf("en", { ...bare, status: "failed" })).toBe(steps("en", "Tried to look online."));
});

test("ChatGPT's search of one site reads as Claude's does", () => {
  const q = "site:indiacode.nic.in Companies Act section 248";
  const done = { site: "indiacode.nic.in", query: "Companies Act section 248" };
  expect(doneOf("en", chatgpt("completed", false, q))).toBe(
    steps("en", "Searched {site} for '{query}'.", done),
  );
  expect(doneOf("en", claude("completed", q))).toBe(
    steps("en", "Searched {site} for '{query}'.", done),
  );
});

test("ChatGPT opening a page reads as reading that site online", () => {
  const open = entry("Open page: https://www.livelaw.in/x", "search", "completed", {
    type: "webSearch",
    id: "ws_2",
    query: "",
    action: { type: "openPage", url: "https://www.livelaw.in/x" },
  });
  expect(doneOf("en", open)).toBe(steps("en", "Read {name} online.", { name: "livelaw.in" }));
});

test.each([
  ["Claude's Grep", entry("grep notice", "search", "completed", { pattern: "notice" })],
  ["Claude's Glob", entry("Find **/*.pdf", "search", "completed", { pattern: "**/*.pdf" })],
  [
    "a search with a query and no action",
    entry("Search", "search", "completed", { query: "notice" }),
  ],
] as const)("%s, a search of the papers, keeps the papers' words", (_name, call) => {
  expect(webSearchOf(call)).toBe(false);
  const q = call.raw_input!.inline!.includes("pattern")
    ? (JSON.parse(call.raw_input!.inline!) as { pattern: string }).pattern
    : "notice";
  expect(doneOf("en", call)).toBe(steps("en", "Searched for '{query}'.", { query: q }));
  expect(summaryOf("en", [call])).toBe("Searched the papers.");
  expect(drawnKindOf(call)).toBe("search");
});
