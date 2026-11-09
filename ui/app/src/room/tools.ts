// The tool line's reading of the open thread's header: which of Clauseo's own tools the conversation has, the state
// each glyph shows, and the words that say it, as the line's accessible name and tooltip. A tool this version
// doesn't know, an availability it can't read, or a reason it has no words for says nothing: it is left out, and with
// nothing left the line has no tool segment. A notice on a tool that is on (the browser's list of Chrome versions is
// old) rides along for the popover's row; one this version doesn't know is left out, the tool kept.
import type { Lang } from "@clauseo/components/lib/display.ts";
import { toolsLine, type ToolsLineKey } from "@clauseo/components/lib/words-tools-line.ts";
import type { ToolGlyphState } from "@clauseo/components/icons/tool-glyph.tsx";
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
  | "browser_not_installed"
  | "browser_blocked_by_policy"
  | "browser_policy_unreadable"
  | "browser_out_of_date"
  | "browser_list_unavailable"
  | "browser_list_expired";

/** A note on a tool that is on, which takes nothing away from it: the newest list of Chrome versions Clauseo has is
 * old, so a newer Chrome may be out. */
export type BrowserNotice = "browser_list_old";

/** One tool the line can speak for. */
export type KnownTool =
  | { tool: "web_search"; state: WebState }
  | { tool: "browser"; state: BrowserState; notice?: BrowserNotice };

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
  "browser_blocked_by_policy",
  "browser_policy_unreadable",
  "browser_out_of_date",
  "browser_list_unavailable",
  "browser_list_expired",
]);
const BROWSER_NOTICES: ReadonlySet<string> = new Set<BrowserNotice>(["browser_list_old"]);

/** A browser that is on's notice, or null: none, one this version doesn't know, or the browser off. */
function noticeOf(t: ToolState, state: string): BrowserNotice | null {
  if (state !== "available") return null;
  const notice: unknown = (t as { notice?: unknown }).notice;
  return typeof notice === "string" && BROWSER_NOTICES.has(notice)
    ? (notice as BrowserNotice)
    : null;
}

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
      if (state !== null) {
        const notice = noticeOf(t, state);
        browser =
          notice === null
            ? { tool: "browser", state: state as BrowserState }
            : { tool: "browser", state: state as BrowserState, notice };
      }
    }
  }
  return [web, browser].filter((t): t is KnownTool => t !== null);
}

/** The state a tool's glyph shows: on as drawn, the firm's lock for a tool the firm holds off
 * (its browser policy too), every other off slashed (a login with no search shows web search off:
 * it can't search here; a browser policy Clauseo couldn't read is no firm's lock). */
export function glyphState(t: KnownTool): ToolGlyphState {
  if (t.state === "available") return "on";
  return t.state === "locked_off" ||
    t.state === "managed_by_firm" ||
    t.state === "browser_blocked_by_policy"
    ? "locked"
    : "off";
}

const REASON: Record<Exclude<WebState | BrowserState, "available">, ToolsLineKey> = {
  off: "In Settings",
  locked_off: "Managed by firm",
  managed_by_firm: "Managed by firm",
  no_search_on_this_login: "Can't search on this login",
  no_proxy: "Not available yet",
  too_many_browsers: "Too many browsers open",
  browser_not_installed: "Needs Chrome",
  browser_blocked_by_policy: "Blocked by your firm",
  browser_policy_unreadable: "Couldn't check browser settings",
  browser_out_of_date: "Needs a newer Chrome",
  browser_list_unavailable: "Couldn't check Chrome's version",
  browser_list_expired: "Can't update Chrome right now",
};

/** Why a tool is off, in a few words; null while it's on. */
export const reasonOf = (t: KnownTool): ToolsLineKey | null =>
  t.state === "available" ? null : REASON[t.state];

/** A tool's name. */
export const toolName = (t: KnownTool): ToolsLineKey =>
  t.tool === "web_search" ? "Web search" : "Browser";

/** A reason inside an English sentence: its first letter in lower case, unless the first two letters are both
 * capitals. Hindi has no case, and its reasons may start with a brand name (Chrome), so they stay as they are. */
const midSentence = (lang: Lang, s: string) =>
  lang === "en" && /^\p{Lu}\p{Ll}/u.test(s) ? s.charAt(0).toLocaleLowerCase("en") + s.slice(1) : s;

/** The tool line's accessible name and tooltip: one sentence per tool, "Web search: on. Browser: off, needs
 * Chrome."; null with no tools to speak for. Off in Settings: "Web search: off, in Settings." */
export function toolsLabel(lang: Lang, tools: readonly KnownTool[]): string | null {
  if (tools.length === 0) return null;
  return tools
    .map((t) => {
      const reason = reasonOf(t);
      const state =
        reason === null
          ? toolsLine(lang, "on")
          : toolsLine(lang, "off, {reason}", {
              reason: midSentence(lang, toolsLine(lang, reason)),
            });
      return toolsLine(lang, "{tool}: {state}.", { tool: toolsLine(lang, toolName(t)), state });
    })
    .join(" ");
}

/** A tool's notice, or null. */
export const noticeOfTool = (t: KnownTool): BrowserNotice | null =>
  t.tool === "browser" ? (t.notice ?? null) : null;

/** The same tools, read the same: a header that changes for its status or usage keeps its tools' array. */
export function sameTools(a: readonly KnownTool[], b: readonly KnownTool[]): boolean {
  return (
    a.length === b.length &&
    a.every((t, i) => {
      const u = b[i];
      return (
        u !== undefined &&
        t.tool === u.tool &&
        t.state === u.state &&
        noticeOfTool(t) === noticeOfTool(u)
      );
    })
  );
}
