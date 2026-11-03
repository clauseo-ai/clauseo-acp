// The step group's model: which rows of an answer fold into one group, and the group's words. Everything between two
// of the agent's texts (its tool calls and its thoughts) folds into one group; a run of thoughts alone is no group and
// stays as its thought rows. A permission prompt, a question for the lawyer, a turn's end, a notice or a steer stands
// on its own and parts the runs either side of it. Pure, so the live thread and an older page group alike.
import type { Lang } from "@clauseo/components/lib/display.ts";
import { steps, type StepKey } from "@clauseo/components/lib/words-steps.ts";
import { stepLive } from "@clauseo/components/lib/words-steps-live.ts";
import { mt } from "@clauseo/components/lib/words-matters.ts";
import { th } from "@clauseo/components/lib/words-thread.ts";
import { stepKindOf, type StepKind } from "@clauseo/components/clauseo/tool-kinds.ts";
import type { StreamView } from "../../store/view.ts";
import type { EntryId, ToolCallEntry, ViewEntry } from "../../transport/protocol.ts";
import { sameIds } from "./ids.ts";
import { doingOf, placeOf, sentenceOf, titleOf, unsettled } from "./step-names.ts";

/** One part of an answer: a row on its own, or a group of rows folded into one line. */
export type Segment =
  | { readonly row: EntryId; readonly group?: never }
  | { readonly group: readonly EntryId[]; readonly row?: never };

/** How a row takes part: a tool call, a thought, or a row that stands alone. */
export type Part = "tool" | "thought" | "alone";

const TOOL = "tool_call:";
const TEXT = "agent_text:";

/** The answer's rows, in order, with every run of tool calls and thoughts holding a tool call as one group. */
export function segmentsOf(ids: readonly EntryId[], partOf: (id: EntryId) => Part): Segment[] {
  const out: Segment[] = [];
  let run: EntryId[] = [];
  let tools = false;
  const close = () => {
    if (tools) out.push({ group: run });
    else for (const id of run) out.push({ row: id });
    run = [];
    tools = false;
  };
  for (const id of ids) {
    const part = partOf(id);
    if (part === "alone") {
      close();
      out.push({ row: id });
    } else {
      run.push(id);
      if (part === "tool") tools = true;
    }
  }
  close();
  return out;
}

/** A row's part from its entry. */
export function partOfEntry(entry: ViewEntry | undefined): Part {
  if (entry?.tool_call !== undefined) return "tool";
  if (entry?.agent_text?.kind === "thought") return "thought";
  return "alone";
}

/** An exchange's segments from an older page (`paged`) or the view, the same array for as long as its ids are the
 * same: a thought's kind never changes, so the ids alone say when to look again, and a flush of text renders nothing. */
export function segmentsPick(
  ids: readonly EntryId[],
  paged: ReadonlyMap<EntryId, ViewEntry> = new Map(),
): (v: StreamView | undefined) => Segment[] {
  let seen: readonly EntryId[] | null = null;
  let last: Segment[] = [];
  let ready = false;
  return (v) => {
    if (ready && seen !== null && sameIds(seen, ids)) return last;
    seen = ids;
    // a row not in yet reads as alone; it comes in with the ids, so this is only a window still filling
    last = segmentsOf(ids, (id) =>
      id.startsWith(TOOL)
        ? "tool"
        : id.startsWith(TEXT)
          ? partOfEntry(paged.get(id) ?? v?.entries.get(id))
          : "alone",
    );
    ready = v !== undefined;
    return last;
  };
}

/** The words of a summary sentence for a kind, one and many. */
const COUNTED: Record<Exclude<StepKind, "think">, [one: StepKey, many: StepKey]> = {
  read: ["1 paper read.", "{n} papers read."],
  edit: ["1 paper changed.", "{n} papers changed."],
  delete: ["1 paper deleted.", "{n} papers deleted."],
  move: ["1 paper moved.", "{n} papers moved."],
  search: ["Searched once.", "Searched {n} times."],
  fetch: ["Looked online once.", "Looked online {n} times."],
  execute: ["Ran 1 task.", "Ran {n} tasks."],
  other: ["Did 1 other thing.", "Did {n} other things."],
};

const counted = (lang: Lang, [one, many]: [StepKey, StepKey], n: number) =>
  n === 1 ? steps(lang, one) : steps(lang, many, { n });

/** The kinds of a group's steps, the most first, ties in the order each came: the folded line's icons. */
export function kindsOf(calls: readonly ToolCallEntry[]): { kind: StepKind; n: number }[] {
  const by = new Map<StepKind, number>();
  for (const c of calls) {
    const k = stepKindOf(c.kind);
    by.set(k, (by.get(k) ?? 0) + 1);
  }
  // Map keeps the order each kind came; sort is stable
  return [...by].map(([kind, n]) => ({ kind, n })).sort((a, b) => b.n - a.n);
}

/** The folded line's icons, by kindsOf: other draws switch_mode's own glyph when every step it counts switched the
 * mode (the words still count it as other). */
export function iconsOf(calls: readonly ToolCallEntry[]): string[] {
  const modes = calls.every((c) => stepKindOf(c.kind) !== "other" || c.kind === "switch_mode");
  return kindsOf(calls).map((k) => (k.kind === "other" && modes ? "switch_mode" : k.kind));
}

/** The most kinds the folded line names; the rest are "And 3 more.". */
const NAMED = 3;

/** The folded line's summary: one sentence for each of the three kinds done most, "4 papers read. Searched 2 times.
 * Ran 1 task.", the rest counted as "And 3 more.". */
export function summaryOf(lang: Lang, calls: readonly ToolCallEntry[]): string {
  const kinds = kindsOf(calls);
  const out = kinds
    .slice(0, NAMED)
    .map(({ kind, n }) =>
      kind === "think" ? steps(lang, "Planned the work.") : counted(lang, COUNTED[kind], n),
    );
  const rest = kinds.slice(NAMED).reduce((sum, k) => sum + k.n, 0);
  if (rest > 0) out.push(counted(lang, ["And 1 more.", "And {n} more."], rest));
  return out.join(" ");
}

/** The folded line for a group at rest. Its words and its icons count only the steps that finished, so a step still
 * waiting, or one that failed or was stopped, is never told as done. With none finished, the newest step still to
 * finish says what it is doing. With every step over and none finished, what was tried ("Tried to read Lease.pdf.",
 * up to three, the rest as more) is the trouble itself, said once and led by the break, never followed by "It failed.". */
export function restOf(
  lang: Lang,
  calls: readonly ToolCallEntry[],
): { kinds: string[]; line: string; trouble: string } {
  const done = calls.filter((c) => c.status === "completed");
  if (done.length > 0)
    return { kinds: iconsOf(done), line: summaryOf(lang, done), trouble: troubleOf(lang, calls) };
  const waiting = [...calls].reverse().find(unsettled);
  if (waiting !== undefined)
    return {
      kinds: iconsOf([waiting]),
      line: doingOf(lang, waiting),
      trouble: troubleOf(lang, calls),
    };
  const out = calls.slice(0, NAMED).map((c) => doneOf(lang, c));
  const rest = calls.length - NAMED;
  if (rest > 0) out.push(counted(lang, ["And 1 more.", "And {n} more."], rest));
  // the break leads what was tried, alone: a kind icon before it would mark the one step twice
  return { kinds: [], line: "", trouble: out.join(" ") };
}

/** What failed or was stopped in a group with a step that finished or is still to, in words for the folded line; ""
 * when nothing did. A group with every step over and none finished says what it tried instead (restOf). */
export function troubleOf(lang: Lang, calls: readonly ToolCallEntry[]): string {
  const failed = calls.filter((c) => c.status === "failed").length;
  const stopped = calls.filter((c) => c.status === "cancelled").length;
  const out: string[] = [];
  if (failed > 0) out.push(counted(lang, ["1 of them failed.", "{n} of them failed."], failed));
  if (stopped > 0)
    out.push(counted(lang, ["1 of them was stopped.", "{n} of them were stopped."], stopped));
  return out.join(" ");
}

/** The folded line's muted words for the steps a steer moved to the background (U7); "" for none. */
export function backgroundLine(lang: Lang, calls: readonly ToolCallEntry[]): string {
  const n = calls.filter((c) => c.background !== undefined).length;
  if (n === 0) return "";
  return calls.length === 1
    ? steps(lang, "It went on in the background.")
    : counted(
        lang,
        ["1 of them went on in the background.", "{n} of them went on in the background."],
        n,
      );
}

/** The live line's count of the steps done so far; "" before the first. */
export function doneSoFar(lang: Lang, calls: readonly ToolCallEntry[]): string {
  const n = calls.filter((c) => c.status === "completed").length;
  return n === 0 ? "" : counted(lang, ["1 done so far.", "{n} done so far."], n);
}

/** A row's words: what it did once done ("Read Lease.pdf."), what it set out to do once it failed or was stopped
 * ("Tried to read Lease.pdf.", its mark saying how it ended), and what it is doing while it waits or runs. */
export function doneOf(lang: Lang, entry: ToolCallEntry): string {
  if (unsettled(entry)) return doingOf(lang, entry);
  const tried = entry.status !== "completed";
  const say = (done: StepKey, attempt: StepKey, vars?: Record<string, string>) =>
    steps(lang, tried ? attempt : done, vars);
  const p = placeOf(entry);
  const name = p.name;
  // the agent's own words where nothing else names it: as they are once done, after the attempt once not
  const titled = (plain: string) => {
    const own = titleOf(entry.title);
    return own === "" ? plain : agentWords(own);
  };
  const agentWords = (own: string) =>
    tried
      ? steps(lang, "Tried: {title}.", { title: own.replace(/[.!?।…]+$/, "") })
      : sentenceOf(own);
  switch (entry.kind) {
    case "read":
      return name === null
        ? titled(say("Read the papers.", "Tried to read the papers."))
        : say("Read {name}.", "Tried to read {name}.", { name });
    case "edit":
      return name === null
        ? titled(say("Changed a paper.", "Tried to change a paper."))
        : say("Changed {name}.", "Tried to change {name}.", { name });
    case "delete":
      return name === null
        ? titled(say("Deleted a paper.", "Tried to delete a paper."))
        : say("Deleted {name}.", "Tried to delete {name}.", { name });
    case "move":
      return name === null
        ? titled(say("Moved a paper.", "Tried to move a paper."))
        : say("Moved {name}.", "Tried to move {name}.", { name });
    case "search":
      return p.query === null
        ? titled(say("Searched the papers.", "Tried to search the papers."))
        : say("Searched for '{query}'.", "Tried to search for '{query}'.", { query: p.query });
    case "fetch":
      return p.query !== null
        ? say("Searched online for '{query}'.", "Tried to search online for '{query}'.", {
            query: p.query,
          })
        : name !== null
          ? say("Read {name} online.", "Tried to read {name} online.", { name })
          : titled(say("Looked online.", "Tried to look online."));
    case "execute":
      return p.what === null ? say("Ran a task.", "Tried to run a task.") : agentWords(p.what);
    case "think":
      return say("Thought it through.", "Tried to think it through.");
    default:
      return titled(say("Did one other thing.", "Tried one other thing."));
  }
}

/** A row's hover: where the paper is, the folder within the matter or the place outside it; null with none. */
export function hoverOf(entry: ToolCallEntry): string | null {
  return placeOf(entry).where;
}

/** A row's state where it didn't simply finish. */
export function markOf(lang: Lang, call: ToolCallEntry): string | null {
  if (call.status === "failed") return th(lang, "Failed");
  if (call.status === "cancelled") return th(lang, "Stopped");
  if (call.background !== undefined) return mt(lang, "In the background");
  if (call.status === "pending") return steps(lang, "Waiting");
  return null;
}

/** The group's line: the summary, or live what is being done now with the count done so far. */
export function lineOf(lang: Lang, entries: readonly (ViewEntry | undefined)[], live: boolean) {
  const calls: ToolCallEntry[] = [];
  for (const e of entries) if (e?.tool_call !== undefined) calls.push(e.tool_call);
  const aside = backgroundLine(lang, calls);
  if (!live) return { ...restOf(lang, calls), more: aside };
  // the newest of what is still to finish, or a thought after the last of them
  let now: ToolCallEntry | "thought" | null = null;
  for (let i = entries.length - 1; i >= 0 && now === null; i -= 1) {
    const e = entries[i];
    if (e?.agent_text !== undefined) now = "thought";
    else if (e?.tool_call !== undefined && unsettled(e.tool_call)) now = e.tool_call;
    else if (e?.tool_call !== undefined) break;
  }
  if (now === null) return { ...restOf(lang, calls), more: aside };
  return {
    kinds: now === "thought" ? ["think"] : [now.kind as string],
    line: now === "thought" ? stepLive(lang, "Thinking it through.") : doingOf(lang, now),
    more: [doneSoFar(lang, calls), aside].filter((m) => m !== "").join(" "),
    trouble: troubleOf(lang, calls),
  };
}
