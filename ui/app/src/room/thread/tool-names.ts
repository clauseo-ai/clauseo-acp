// A tool's or a permission's own name as an agent sometimes sends it for a title ("SandboxNetworkAccess", "write_file",
// "mcp__browser__navigate", "mcp.browser.agent_browser_open", "fs.read", "ApplyPatch"): never shown as it came. A name
// Clauseo knows reads as what it does, in the room's words. Once a step is over, any other name is shown made readable
// (readableTool: no mcp__ or mcp. prefix, underscores and capitals to spaces, an MCP tool led by its server's name, as
// "Indian Kanoon: search judgments"), so a step never hides behind "other things" (issue 13; until 24 Nov such a name
// read as the kind's plain line). A permission prompt still shows nothing beneath "Clauseo asks before going on.". A
// title in words ("Read Notice to the bank.pdf") stays as it is.
// Pure, and without words of its own: each place that tells a step or a prompt holds its words for each name
// (step-names.ts while a step runs, tool-words.ts once it is over, permission-model.ts for a prompt), so the room's
// first chunk takes none of the settled group's.

import type { ToolCallEntry } from "../../transport/protocol.ts";

/** What a known name does, as the room tells it. */
export type ToolAs = "network" | "execute" | "read" | "edit" | "search" | "fetch";

/** A paper's extension: a title that is one file's name ("brief.txt", "Lease.pdf") is shown on purpose. */
const PAPER =
  /\.(pdf|docx?|rtf|odt|txt|md|xlsx?|csv|pptx?|eml|msg|html?|jpe?g|png|tiff?|json|xml|zip)$/i;

/** Whether a title is shaped like a name rather than words: no space, and CamelCase ("ApplyPatch"), or snake_case,
 * kebab-case, dotted or with a colon ("write_file", "read-file", "fs.read", "mcp:fetch"), or a capital inside a run
 * of letters ("readFile"). An ordinary word ("Read", "PDF") and a paper's file name ("brief.txt") are not. */
export function identifierLike(title: string): boolean {
  const t = title.trim();
  if (t === "" || !/^[A-Za-z0-9_.:-]+$/.test(t) || PAPER.test(t)) return false;
  return (
    /^[A-Z][a-z0-9]+(?:[A-Z][a-z0-9]+)+$/.test(t) ||
    /[_.:]/.test(t) ||
    /^[a-z0-9]+(?:-[a-z0-9]+)+$/.test(t) ||
    /[a-z0-9][A-Z]/.test(t)
  );
}

/** The names agents send, folded (lower case, no `_ . : -`), by what they do: Claude's tools, ChatGPT's (Codex's), the
 * file system's and the sandbox's permission. */
const KNOWN: Readonly<Record<string, ToolAs>> = {
  sandboxnetworkaccess: "network",
  networkaccess: "network",
  network: "network",
  bash: "execute",
  bashoutput: "execute",
  killshell: "execute",
  shell: "execute",
  localshell: "execute",
  execcommand: "execute",
  writestdin: "execute",
  runcommand: "execute",
  exec: "execute",
  terminal: "execute",
  read: "read",
  readfile: "read",
  readtextfile: "read",
  fsread: "read",
  fsreadtextfile: "read",
  viewimage: "read",
  write: "edit",
  writefile: "edit",
  writetextfile: "edit",
  fswrite: "edit",
  fswritetextfile: "edit",
  edit: "edit",
  editfile: "edit",
  multiedit: "edit",
  applypatch: "edit",
  notebookedit: "edit",
  grep: "search",
  glob: "search",
  websearch: "fetch",
  webfetch: "fetch",
};

/** The browser's tools, by their MCP server: looking online. Claude (claude-agent-acp) titles an MCP call with its
 * name, `mcp__<server>__<tool>` ("mcp__browser__agent_browser_open"); ChatGPT (codex-acp) with
 * `mcp.<server>.<tool>` ("mcp.browser.agent_browser_open"). */
const BROWSER = /^(?:mcp__browser__|mcp\.browser\.)/i;

/** What a tool of Clauseo's case-law connector does, as the room tells it: searching case law (a search, a judgment
 * read, a citation checked), checking the lawyer's case-law credits (it only reads them), or getting a payment link for
 * more (it never pays: the lawyer pays on the payment page). */
export type CaseLaw = "search" | "credits" | "payment";

/** The case-law connector's tools, by their exact names (crates/host/src/agent/loads/case_law.rs). A tool of it not
 * named here reads as any other tool's name does. */
const CASE_LAW_TOOLS: Readonly<Record<string, CaseLaw>> = {
  search_law: "search",
  read_document: "search",
  citator: "search",
  check_citations: "search",
  account: "credits",
  recharge: "payment",
};

/** A call of the case-law connector, its server `case-law`, as Claude names it ("mcp__case-law__search_law") or
 * ChatGPT does ("mcp.case-law.search_law"): its tool's name. */
const CASE_LAW = /^(?:mcp__case-law__|mcp\.case-law\.)([a-z_]+)$/;

/** What a title naming a case-law tool does; null for any other title. */
export function caseLawOf(title: string): CaseLaw | null {
  const tool = CASE_LAW.exec(title.trim().replace(/^`+|`+$/g, ""))?.[1];
  return tool !== undefined && Object.hasOwn(CASE_LAW_TOOLS, tool) ? CASE_LAW_TOOLS[tool]! : null;
}

/** A title that is a tool's name: what it does when Clauseo knows it (`as`), null when it doesn't; undefined for a
 * title in words, which is shown. A known name counts in any shape, "Bash" as much as "exec_command". */
export function toolNameOf(title: string): { as: ToolAs | null } | undefined {
  const t = title.trim().replace(/^`+|`+$/g, "");
  if (/\s/.test(t) || t === "") return undefined;
  if (BROWSER.test(t)) return { as: "fetch" };
  const as = KNOWN[t.toLowerCase().replace(/[_.:-]/g, "")];
  if (as !== undefined) return { as };
  return identifierLike(t) ? { as: null } : undefined;
}

/** Whether a step is Claude's Yield (Claude Code 2.1.330): its own wait for its background work (helpers, background
 * commands, Monitor), read-only. Told as waiting, never as another thing or a tool used. */
export const yieldOf = (call: Pick<ToolCallEntry, "title" | "tool_name">): boolean =>
  call.tool_name?.toLowerCase() === "yield" || call.title.trim() === "Yield";

/** An MCP server's name as the lawyer reads it, where its own spelling says less: by the name folded (lower case,
 * no `_ . : -` or spaces). Any other server is named by its own name made readable (readableServer). */
const SERVERS: Readonly<Record<string, string>> = {
  indiankanoon: "Indian Kanoon",
  ikapi: "Indian Kanoon",
  scconline: "SCC Online",
  manupatra: "Manupatra",
  livelaw: "LiveLaw",
};

/** A name's words: its mcp prefix off, CamelCase, `_ . : -` and runs of space to one space. */
const wordsOfName = (name: string): string[] =>
  name
    .replace(/^mcp(?:__|\.)/i, "")
    .replace(/([a-z0-9])([A-Z])/g, "$1 $2")
    .split(/[\s_.:-]+/)
    .filter((w) => w !== "");

/** An MCP server's readable name: a known one's (SERVERS), else its words, each lower-case one capitalised
 * ("indian-kanoon" → "Indian Kanoon", "SCC" stays). */
export function readableServer(server: string): string {
  const known = SERVERS[server.toLowerCase().replace(/[\s_.:-]/g, "")];
  if (known !== undefined) return known;
  return wordsOfName(server)
    .map((w) => (/^[a-z]/.test(w) ? w[0]!.toUpperCase() + w.slice(1) : w))
    .join(" ");
}

/** A tool's readable name: its words in lower case, an acronym kept ("write_file" → "write file", "TodoWrite" →
 * "todo write", "get_PDF" → "get PDF"). */
export const readableTool = (tool: string): string =>
  wordsOfName(tool)
    .map((w) => (/^[A-Z][a-z0-9]*$/.test(w) ? w.toLowerCase() : w))
    .join(" ");

/** Claude's MCP title, `mcp__<server>__<tool>`, and ChatGPT's, `mcp.<server>.<tool>`. */
const MCP = /^mcp(?:__(.+?)__(.+)|\.([^.]+)\.(.+))$/;

/** An MCP call of a server other than Clauseo's own (the browser's and the case-law connector's, which say what they
 * do): its server and tool as they came, from the title, or from ChatGPT's input `{server, tool}`; null for any other
 * call. */
export function mcpOf(title: string, raw?: string): { server: string; tool: string } | null {
  const t = title.trim().replace(/^`+|`+$/g, "");
  const m = MCP.exec(t);
  let server = m?.[1] ?? m?.[3] ?? null;
  let tool = m?.[2] ?? m?.[4] ?? null;
  if (server === null && raw !== undefined && raw.length < 16_384 && raw.startsWith("{")) {
    try {
      const v = JSON.parse(raw) as Record<string, unknown>;
      if (typeof v["server"] === "string" && typeof v["tool"] === "string" && /^mcp\b/i.test(t)) {
        server = v["server"];
        tool = v["tool"];
      }
    } catch {
      // no input to read: no server
    }
  }
  if (server === null || tool === null || server === "browser" || server === "case-law")
    return null;
  return { server, tool };
}

/** A step's tool as the lawyer reads it, for a step nothing else names: an MCP tool by its server and tool
 * ("Indian Kanoon: search judgments"), a title shaped like a name made readable, else the call's tool name made
 * readable; null with none of these. */
export function toolLabelOf(
  call: Pick<ToolCallEntry, "title" | "tool_name" | "raw_input">,
): string | null {
  const mcp = mcpOf(call.title, call.raw_input?.inline);
  if (mcp !== null) return `${readableServer(mcp.server)}: ${readableTool(mcp.tool)}`;
  const t = call.title.trim().replace(/^`+|`+$/g, "");
  if (toolNameOf(t)?.as === null) return readableTool(t);
  const name = call.tool_name?.trim() ?? "";
  return name === "" ? null : readableTool(name);
}
