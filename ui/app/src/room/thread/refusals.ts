// A step Claude's own checks refused before it ran, as the lawyer reads it (#plain-refusal). Claude Code answers a
// refused tool call with an error result that claude-agent-acp puts in the call's raw output (and, fenced, in its
// content); without this the row's why line (task-words.ts's whyOf) would show that text cut at 120 characters:
// "Permission for this action was denied by the Claude Code auto mode classifier. Reason: [Unauthorized Access] …".
// Three kinds, by the exact text of Claude Code 0.3.293 (claude-agent-sdk-linux-x64's package/claude):
// - its auto mode safety check blocked it: "… denied by the Claude Code auto mode classifier. Reason: [<rule>] …",
//   the rule's name in square brackets (or "Permission for this action has been denied. Reason: [<rule>]");
// - that check gave no verdict ("… was not reviewed: …", "… auto mode cannot determine the safety of …");
// - one of Clauseo's own deny rules matched: "Permission to read <path> has been denied." (use, edit, write).
// Pure, and only the words change: what Claude itself receives is untouched, and the raw text stays in the call.
import type { Lang } from "@clauseo/components/lib/display.ts";
import { refusal, type RefusalKey } from "@clauseo/components/lib/words-refusals.ts";
import type { ToolCallEntry } from "../../transport/protocol.ts";

/** What the safety check's rule name says the action looked like; `other` for a name none of these knows. */
export type Looked =
  | "access"
  | "outside_data"
  | "credentials"
  | "deletion"
  | "oversight"
  | "self"
  | "outside_change"
  | "instructions"
  | "untrusted_code"
  | "spending"
  | "other";

export type Refusal = { kind: "check"; looked: Looked } | { kind: "no_verdict" } | { kind: "rule" };

/** Past this, a raw output isn't read: a refusal is a paragraph, never a file's text. */
const RAW_MAX = 16_384;
/** The safety check's block, then the shorter form Claude Code counts as the same (its `p2t`). */
const CHECK = [
  "denied by the Claude Code auto mode classifier. Reason: ",
  "Permission for this action has been denied. Reason: ",
];
/** Claude Code 2.1.293's built-in safety check on a command: it names no rule, so it reads as the general line. */
const SAFETY_CHECK = "denied by a built-in Claude Code safety check";
/** The check's own words when it gave no verdict: busy, refused, too long a transcript, nothing to judge. Its
 * phrases only, so a task whose output says "classifier" is never taken for one. */
const NO_VERDICT =
  /auto mode (?:classifier|cannot determine|could not evaluate)|auto mode's (?:safety classifier|review)|not a judgment that the (?:action|call) is unsafe/i;
/** Claude Code's own deny rule ("Permission to read …/agents/… has been denied."). */
const RULE = /^Permission to (?:use|read|edit|write)\b[\s\S]* has been denied\b/;

/** A rule name's words, in order: the first match names what it looked like. Claude Code ships no list of its
 * rules' names (the model names one per block), so these read its words; the binary's own examples and the ones
 * seen in Clauseo are in refusals.test.ts. */
const LOOKED: readonly (readonly [RegExp, Looked])[] = [
  [/poison|injection|instruction/, "instructions"],
  [/credential|secret|password|token/, "credentials"],
  [/exfil|leak|scouting|upload/, "outside_data"],
  [/delete|deletion|destruct|irreversible|wipe|overwrite/, "deletion"],
  [/oversight|audit|logging|bypass|weaken|tamper|monitor/, "oversight"],
  [/self|persistence|permission grant|unsafe agent/, "self"],
  [/untrusted|external code|code from|\brce\b|guessed|download/, "untrusted_code"],
  [/transaction|purchase|payment|spend/, "spending"],
  [/access|unauthori|scope/, "access"],
  [/external|production|deploy|push|shared|interfere|remote|write|modif/, "outside_change"],
];

/** What a rule name ("Unauthorized Access", "Auto-Mode Bypass") says the action looked like. */
export function lookedOf(rule: string): Looked {
  const words = rule.toLowerCase().replace(/[^a-z]+/g, " ");
  return LOOKED.find(([re]) => re.test(words))?.[1] ?? "other";
}

/** The text Claude Code sent back: the raw output's string, or its text blocks', else the content's text with its
 * fence off. */
function sentOf(entry: ToolCallEntry): string | null {
  const raw = entry.raw_output?.inline;
  if (raw !== undefined && raw.length <= RAW_MAX) {
    let v: unknown;
    try {
      v = JSON.parse(raw) as unknown;
    } catch {
      v = raw;
    }
    if (typeof v === "string") return v;
    if (Array.isArray(v)) {
      const texts = v.flatMap((b: unknown) =>
        typeof b === "object" && b !== null && typeof (b as { text?: unknown }).text === "string"
          ? [(b as { text: string }).text]
          : [],
      );
      if (texts.length > 0) return texts.join("\n");
    }
  }
  for (const c of entry.content) {
    const text = typeof c === "object" ? c.text?.text.inline : undefined;
    if (text !== undefined && text.length <= RAW_MAX)
      return text.replace(/^\s*`{3,}[^\n]*\n|\n`{3,}\s*$/g, "");
  }
  return null;
}

/** The refusal a failed call's result holds, or null for any other result. */
export function refusalOf(entry: ToolCallEntry): Refusal | null {
  if (entry.status !== "failed") return null;
  const sent = sentOf(entry)
    ?.replace(/<\/?tool_use_error>/g, "")
    .trim();
  if (sent === undefined || sent === "") return null;
  for (const lead of CHECK) {
    const at = sent.indexOf(lead);
    if (at < 0) continue;
    const rule = /^\[([^\]\n]{1,80})\]/.exec(sent.slice(at + lead.length));
    return { kind: "check", looked: rule === null ? "other" : lookedOf(rule[1]!) };
  }
  if (sent.includes(SAFETY_CHECK)) return { kind: "check", looked: "other" };
  if (RULE.test(sent)) return { kind: "rule" };
  if (NO_VERDICT.test(sent)) return { kind: "no_verdict" };
  return null;
}

const M = "Nothing was changed.";
const LINE: Readonly<Record<Looked, RefusalKey>> = {
  access: `Claude's built-in safety check stopped this: it looked like reaching files or systems Claude wasn't asked to use. ${M}`,
  outside_data: `Claude's built-in safety check stopped this: it looked like sending matter information somewhere outside. ${M}`,
  credentials: `Claude's built-in safety check stopped this: it looked like a search for passwords or sign-in keys. ${M}`,
  deletion: `Claude's built-in safety check stopped this: it looked like deleting or overwriting work in a way that can't be undone. ${M}`,
  oversight: `Claude's built-in safety check stopped this: it looked like getting around the checks on Claude's work. ${M}`,
  self: `Claude's built-in safety check stopped this: it looked like changing how Claude itself is set up. ${M}`,
  outside_change: `Claude's built-in safety check stopped this: it looked like changing something outside this matter. ${M}`,
  instructions: `Claude's built-in safety check stopped this: it looked like following instructions hidden in a document or web page. ${M}`,
  untrusted_code: `Claude's built-in safety check stopped this: it looked like running code from a source that isn't trusted. ${M}`,
  spending: `Claude's built-in safety check stopped this: it looked like spending money or acting for you outside Clauseo. ${M}`,
  other: `Claude's built-in safety check stopped this. ${M}`,
};

/** The plain line for a refused call, in the language; null when the call wasn't refused this way. */
export function refusalWhy(lang: Lang, entry: ToolCallEntry): string | null {
  const r = refusalOf(entry);
  if (r === null) return null;
  if (r.kind === "check") return refusal(lang, LINE[r.looked]);
  if (r.kind === "no_verdict")
    return refusal(
      lang,
      "Claude's built-in safety check couldn't look at this just now, so it didn't run. Nothing was changed.",
    );
  return refusal(
    lang,
    "This tried to reach somewhere Clauseo keeps off limits here, such as Clauseo's own app data or another matter. Nothing was changed.",
  );
}
