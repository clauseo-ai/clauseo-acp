// A tool's or a permission's own name as an agent sometimes sends it for a title ("SandboxNetworkAccess", "write_file",
// "mcp__browser__navigate", "fs.read", "ApplyPatch"): never shown. A name Clauseo knows reads as what it does, in the
// room's words; any other title shaped like a name reads as the kind's own plain line, or, on a permission prompt,
// as nothing beneath "Clauseo asks before going on.". A title in words ("Read Notice to the bank.pdf") stays as it is.
// Pure, and without words of its own: each place that tells a step or a prompt holds its words for each name
// (step-names.ts while a step runs, tool-words.ts once it is over, permission-model.ts for a prompt), so the room's
// first chunk takes none of the settled group's.

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

/** The browser's tools, by their MCP server ("mcp__browser__navigate"): looking online. */
const BROWSER = /^mcp__browser__/i;

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
