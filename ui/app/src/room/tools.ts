// The tool line's reading of the open thread's header: which of Clauseo's own tools the conversation has, and the one
// phrase the status line says for them. A tool this version doesn't know, an availability it can't read, or a
// reason it has no words for says nothing: it is left out, and with nothing left the line has no tool segment.
import type { ToolsLineKey } from "@clauseo/components/lib/words-tools-line.ts";
import type { ToolState } from "../transport/protocol.ts";

/** Where web search stands, in the words the popover has. */
export type WebState = "available" | "off" | "locked_off" | "no_search_on_this_login";

/** Where the browser stands, in the words the popover has. */
export type BrowserState =
  | "available"
  | "off"
  | "locked_off"
  | "managed_by_firm"
  | "no_proxy"
  | "too_many_browsers"
  | "browser_not_installed";

/** One tool the line can speak for. */
export type KnownTool =
  | { tool: "web_search"; state: WebState }
  | { tool: "browser"; state: BrowserState };

const WEB: ReadonlySet<string> = new Set<WebState>([
  "off",
  "locked_off",
  "no_search_on_this_login",
]);
const BROWSER: ReadonlySet<string> = new Set<BrowserState>([
  "off",
  "locked_off",
  "managed_by_firm",
  "no_proxy",
  "too_many_browsers",
  "browser_not_installed",
]);

/** The state of a tool, or null where it says nothing: an availability string it doesn't know, a reason it doesn't
 * know or has no words for on that tool. */
function stateOf(
  availability: ToolState["availability"],
  reasons: ReadonlySet<string>,
): string | null {
  if (availability === "available") return "available";
  if (typeof availability !== "object" || availability === null) return null;
  const reason = (availability as { unavailable?: { reason?: unknown } }).unavailable?.reason;
  return typeof reason === "string" && reasons.has(reason) ? reason : null;
}

/** The tools the line speaks for, web search first, each once (its first state); [] for a header with none. */
export function knownTools(tools: readonly ToolState[] | undefined): KnownTool[] {
  let web: KnownTool | null = null;
  let browser: KnownTool | null = null;
  for (const t of tools ?? []) {
    if (t.tool === "web_search" && web === null) {
      const state = stateOf(t.availability, WEB);
      if (state !== null) web = { tool: "web_search", state: state as WebState };
    } else if (t.tool === "browser" && browser === null) {
      const state = stateOf(t.availability, BROWSER);
      if (state !== null) browser = { tool: "browser", state: state as BrowserState };
    }
  }
  return [web, browser].filter((t): t is KnownTool => t !== null);
}

/** The status line's phrase for the tools; null with none to speak for. */
export function toolsSummary(tools: readonly KnownTool[]): ToolsLineKey | null {
  const web = tools.find((t) => t.tool === "web_search")?.state ?? null;
  const browser = tools.find((t) => t.tool === "browser")?.state ?? null;
  const browserOn = browser === "available";
  if (web === null) return browser === null ? null : browserOn ? "Browser on" : "Browser off";
  if (web === "available") return browserOn ? "Web search and browser on" : "Web search on";
  if (web === "no_search_on_this_login")
    return browserOn ? "Web pages and browser on" : "Web pages only";
  return browserOn ? "Browser on, no web search" : "Web search off";
}

/** The same tools, read the same: a header that changes for its status or usage keeps its tools' array. */
export function sameTools(a: readonly KnownTool[], b: readonly KnownTool[]): boolean {
  return (
    a.length === b.length && a.every((t, i) => t.tool === b[i]?.tool && t.state === b[i]?.state)
  );
}
