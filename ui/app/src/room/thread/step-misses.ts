// A page the agent couldn't open, and a step Claude's own safety check held back: neither is a failure the answer
// rests on, so neither is told as "failed" (Windows tester, item C: "6 failed" stood for four HTTP 403s, a 404, a
// DNS miss, a refused connection, a page over 10 MB and two folder listings the safety check refused). A page that
// couldn't open is counted as such ("Couldn't open 4 pages.") and its row gives a plain reason ("Couldn't open
// livelaw.in: the site refused."); a held-back step says so. "Failed" stays for a paper not read, an edit not saved, a
// command that broke. Pure.
import type { Lang } from "@clauseo/components/lib/display.ts";
import { steps, type StepKey } from "@clauseo/components/lib/words-steps.ts";
import type { ToolCallEntry } from "../../transport/protocol.ts";
import { groupKindOf } from "./group-kind.ts";
import { refusalOf, sentOf } from "./refusals.ts";
import { placeOf } from "./step-names.ts";

/** Why a page couldn't open: the site refused it, it isn't there, it is too large, the site couldn't be reached, the
 * site took too long; `unknown` when the result says nothing of these. */
export type Miss = "refused" | "not_found" | "too_large" | "unreachable" | "slow" | "unknown";

/** The start of a result that says the site refused the fetch: an HTTP 4xx or 5xx. */
const REFUSED = /^The server returned HTTP [45]\d\d\b/;

/** Whether a web fetch the site refused: a completed WebFetch whose result text starts with REFUSED. Claude Code does
 * not fail such a fetch; it returns that text as a normal result, so the adapter reports the call completed. Pinned:
 * the text was read from Claude Code as shipped with `@anthropic-ai/claude-agent-sdk` 0.3.293, under claude-agent-acp
 * 0.89.1 (the pin in crates/agents/src/snapshot/embedded.rs:215). A pin bump re-checks it. Any other text, another
 * tool's, or the same words mid-text, is no refusal. */
export function httpRefused(entry: ToolCallEntry): boolean {
  if (entry.status !== "completed" || entry.tool_name?.toLowerCase() !== "webfetch") return false;
  let shown = "";
  for (const c of entry.content)
    if (typeof c === "object" && c.text !== undefined) {
      shown = c.text.text.inline ?? "";
      break;
    }
  return REFUSED.test(shown);
}

/** A result's words, by what they say of the page, the first match winning. Claude Code's WebFetch fails with its HTTP
 * client's message ("Request failed with status code 404", "getaddrinfo ENOTFOUND …", "connect ECONNREFUSED …",
 * "maxContentLength size of 10485760 exceeded", "timeout of 60000ms exceeded") or returns "The server returned HTTP
 * 403 Forbidden." as its result (httpRefused). */
const WHY: readonly (readonly [RegExp, Miss])[] = [
  [/maxContentLength|too large|exceeds? (?:the )?(?:max|limit|size)|content length/i, "too_large"],
  [/timeout|timed out|ETIMEDOUT|ESOCKETTIMEDOUT/i, "slow"],
  [/(?:HTTP|status code)\s*(?:40[137]|451|429)\b/i, "refused"],
  [/(?:HTTP|status code)\s*(?:404|410)\b/i, "not_found"],
  [
    /ENOTFOUND|EAI_AGAIN|getaddrinfo|\bDNS\b|ECONNREFUSED|ECONNRESET|EHOSTUNREACH|ENETUNREACH|socket hang up|certificate|(?:HTTP|status code)\s*5\d\d\b/i,
    "unreachable",
  ],
];

/** Whether a step opens one page: Claude's WebFetch, or ChatGPT's web search opening a page (`action.type`
 * "openPage"). Only such a step's failure with no reason named is a page that couldn't open; a search or a browser's
 * click that failed with nothing said stays tried and failed. */
const opensPage = (entry: ToolCallEntry): boolean =>
  entry.tool_name?.toLowerCase() === "webfetch" ||
  /"type"\s*:\s*"openPage"/.test(entry.raw_input?.inline ?? "");

/** Why a step that looked online couldn't open its page: the reason its error names (WHY), or `unknown` for a step
 * that opens a page (opensPage) whose result says nothing at all. Null for any other step: one that opened its page, one
 * held back (heldBack), or one whose result says something WHY doesn't know (the lawyer's refusal, a blocked site, any
 * other error), which stays failed with its reason line. */
export function missOf(entry: ToolCallEntry): Miss | null {
  if (groupKindOf(entry) !== "fetch") return null;
  if (entry.status !== "failed" && !httpRefused(entry)) return null;
  if (refusalOf(entry) !== null) return null;
  const said = (sentOf(entry) ?? "").trim();
  if (said === "") return opensPage(entry) ? "unknown" : null;
  return WHY.find(([re]) => re.test(said))?.[1] ?? null;
}

/** Who held a step back before it ran (refusals.ts): Claude's safety check (it refused, or couldn't judge), or
 * Clauseo's own off-limits rule; null for a step nothing held back. */
export function heldBy(entry: ToolCallEntry): "check" | "rule" | null {
  const r = refusalOf(entry);
  return r === null ? null : r.kind === "rule" ? "rule" : "check";
}

/** Whether anything held a step back before it ran (heldBy). */
export const heldBack = (entry: ToolCallEntry): boolean => heldBy(entry) !== null;

/** The kinds whose holding back is no failure: reading, listing or searching the papers, looking online. An edit, a
 * deletion, a move or a command held back stays a failure, with the break. */
const SOFT: ReadonlySet<string> = new Set(["read", "search", "fetch"]);

/** A step held back that is no failure (SOFT): counted beside the line, not in its trouble. */
export const softHeld = (entry: ToolCallEntry): boolean =>
  heldBack(entry) && SOFT.has(groupKindOf(entry));

/** A row's mark for a step held back, agreeing with the reason under it; null for one nothing held back. */
export function heldMark(lang: Lang, entry: ToolCallEntry): string | null {
  const by = heldBy(entry);
  if (by === null) return null;
  return steps(
    lang,
    by === "rule" ? "Clauseo keeps this off limits" : "Held back by Claude's safety check",
  );
}

const REASON: Record<Exclude<Miss, "unknown">, StepKey> = {
  refused: "the site refused",
  not_found: "not found",
  too_large: "too large",
  unreachable: "couldn't reach the site",
  slow: "the site took too long",
};

/** A row that couldn't open its page: the site and the reason, either left out when not known. */
export function missWords(lang: Lang, entry: ToolCallEntry, miss: Miss): string {
  const site = placeOf(entry).name;
  if (miss === "unknown")
    return site === null
      ? steps(lang, "Couldn't open a page.")
      : steps(lang, "Couldn't open {site}.", { site });
  const reason = steps(lang, REASON[miss]);
  return site === null
    ? steps(lang, "Couldn't open a page: {reason}.", { reason })
    : steps(lang, "Couldn't open {site}: {reason}.", { site, reason });
}

/** The folded line's words for the pages that couldn't open and the reads held back (softHeld); "" with none. */
export function setbackLine(lang: Lang, calls: readonly ToolCallEntry[]): string {
  const missed = calls.filter((c) => missOf(c) !== null).length;
  const soft = calls.filter(softHeld);
  const held = soft.filter((c) => heldBy(c) === "check").length;
  const ruled = soft.length - held;
  const out: string[] = [];
  if (missed > 0)
    out.push(
      missed === 1
        ? steps(lang, "Couldn't open 1 page.")
        : steps(lang, "Couldn't open {n} pages.", { n: missed }),
    );
  if (held > 0)
    out.push(
      held === 1
        ? steps(lang, "1 held back by Claude's safety check.")
        : steps(lang, "{n} held back by Claude's safety check.", { n: held }),
    );
  if (ruled > 0)
    out.push(
      ruled === 1
        ? steps(lang, "1 kept off limits by Clauseo.")
        : steps(lang, "{n} kept off limits by Clauseo.", { n: ruled }),
    );
  return out.join(" ");
}
