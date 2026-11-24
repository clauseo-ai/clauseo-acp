// The step group's model: which rows of an answer fold into one group, and the group's words. Everything between two
// of the agent's texts (its tool calls and its thoughts) folds into one group; a run of thoughts alone is no group and
// stays as its thought rows. A permission prompt, a question for the lawyer, a turn's end, a notice or a steer stands
// on its own and parts the runs either side of it. Pure, so the live thread and an older page group alike.
import type { Lang } from "@clauseo/components/lib/display.ts";
import { steps, type StepKey } from "@clauseo/components/lib/words-steps.ts";
import { stepLive } from "@clauseo/components/lib/words-steps-live.ts";
import { mt } from "@clauseo/components/lib/words-matters.ts";
import { th } from "@clauseo/components/lib/words-thread.ts";
import type { StreamView } from "../../store/view.ts";
import type { EntryId, Timestamp, ToolCallEntry, ViewEntry } from "../../transport/protocol.ts";
import { toolCallIdOf } from "../permission-model.ts";
import { sameIds } from "./ids.ts";
import {
  asksWords,
  doingOf,
  internalOf,
  kindOf,
  placeOf,
  sentenceOf,
  siteQuery,
  titleOf,
  unsettled,
  webSearchOf,
} from "./step-names.ts";
import { afterRan, taskOf } from "./task-words.ts";
import {
  helpersWaitOf,
  helperToolOf,
  mcpOf,
  ownToolOf,
  toolLabelOf,
  toolNameOf,
  yieldOf,
  type OwnAs,
} from "./tool-names.ts";
import { caseLawDone, skillDone, toolDone, usedDone } from "./tool-words.ts";
import { skillOf } from "./skill-words.ts";
import { helperOf, helperTaskOf, spawnOf } from "./helper-words.ts";
import { groupKindOf, type GroupKind } from "./group-kind.ts";
import { summaryOf } from "./step-summary.ts";
import { heldMark, httpRefused, missOf, missWords, setbackLine, softHeld } from "./step-misses.ts";

/** One part of an answer: a row on its own, or a group of rows folded into one line. */
export type Segment =
  | { readonly row: EntryId; readonly group?: never }
  | { readonly group: readonly EntryId[]; readonly row?: never };

/** How a row takes part: a tool call, a thought, or a row that stands alone. */
export type Part = "tool" | "thought" | "alone";

const TOOL = "tool_call:";
const TEXT = "agent_text:";
const PERMISSION = "permission:";

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

/** `segments` with each answered prompt whose own tool call is in an earlier group, another
 * group between them, moved back to stand after that group and the prompts already there: an
 * answered prompt reads only "Allowed once.", so it must follow the step it answered, never the
 * next one the agent began before asking (`callOf`: an answered prompt row's tool call; null for
 * a pending prompt, which stays where it came). It never moves ahead of a row that stays (the
 * agent's words, a notice, a pending prompt), so those keep their order. */
export function promptsAtTheirCalls(
  segments: readonly Segment[],
  callOf: (row: EntryId) => EntryId | null,
): Segment[] {
  const out: Segment[] = [];
  for (const s of segments) {
    const call = s.row === undefined ? null : callOf(s.row);
    const at = call === null ? -1 : out.findIndex((o) => o.group?.includes(call) === true);
    if (at < 0 || !out.slice(at + 1).some((o) => o.group !== undefined)) {
      out.push(s);
      continue;
    }
    let to = at + 1;
    while (to < out.length && out[to]?.row !== undefined && callOf(out[to]!.row!) !== null) to += 1;
    // the last row that stays, at or after `to`: the prompt goes no further back than just after it
    for (let i = out.length - 1; i >= to; i -= 1) {
      const kept = out[i]?.row;
      if (kept !== undefined && callOf(kept) === null) {
        to = i + 1;
        break;
      }
    }
    out.splice(to, 0, s);
  }
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
    const entry = (id: EntryId) => paged.get(id) ?? v?.entries.get(id);
    const grouped = segmentsOf(ids, (id) =>
      id.startsWith(TOOL) ? "tool" : id.startsWith(TEXT) ? partOfEntry(entry(id)) : "alone",
    );
    last = promptsAtTheirCalls(grouped, (row) => {
      const asked = row.startsWith(PERMISSION) ? entry(row)?.permission : undefined;
      // a pending prompt stays where it came, by the step still running
      return asked === undefined || asked.state === "pending" ? null : toolCallIdOf(asked);
    });
    ready = v !== undefined;
    return last;
  };
}

/** Whether a step's state is one this version can't read (a newer Clauseo's): neither done nor failed. */
const unknownState = (c: ToolCallEntry) => c.status === "unknown";

/** A step that finished: completed, and not a fetch the site refused (httpRefused). */
const finishedOf = (c: ToolCallEntry) => c.status === "completed" && !httpRefused(c);

/** A page that couldn't open, or a read held back (step-misses.ts: softHeld): told as such, never failed. */
const setbackOf = (c: ToolCallEntry) => missOf(c) !== null || softHeld(c);

/** A step that failed: failed, or a fetch the site refused (httpRefused), but no setback (setbackOf). The Failed tag
 * and the trouble count it. */
const failedOf = (c: ToolCallEntry) => (c.status === "failed" || httpRefused(c)) && !setbackOf(c);

/** A step in a state this version can't read, in no tense: by its place ("Lease.pdf."), its words searched for, or
 * the agent's own words cut down ("Check the share register."); with none, that its state isn't known. */
function namedOf(lang: Lang, entry: ToolCallEntry): string {
  const p = placeOf(entry);
  const own = p.name ?? (p.query === null ? null : `'${p.query}'`) ?? p.what;
  // a tool's name is no words to show (tool-names.ts)
  const title =
    toolNameOf(entry.title) === undefined && skillOf(entry) === null ? titleOf(entry.title) : "";
  if (own !== null) return sentenceOf(own);
  return title === "" ? steps(lang, "State not known.") : sentenceOf(title);
}

export { groupKindOf, type GroupKind } from "./group-kind.ts";
export { papersRead, summaryOf } from "./step-summary.ts";
export { httpRefused } from "./step-misses.ts";

/** The kind a step's icon is drawn for: its GroupKind, every case-law tool as case_law (the case-law glyph), and
 * switch_mode as itself. For a row's icon. */
export function drawnKindOf(call: ToolCallEntry): string {
  const k = groupKindOf(call);
  if (k.startsWith("case_law_")) return "case_law";
  return k === "other" ? kindOf(call) : k;
}

const counted = (lang: Lang, [one, many]: [StepKey, StepKey], n: number) =>
  n === 1 ? steps(lang, one) : steps(lang, many, { n });

/** The kinds of a group's steps, the most first, ties in the order each came: the folded line's icons. */
export function kindsOf(calls: readonly ToolCallEntry[]): { kind: GroupKind; n: number }[] {
  const by = new Map<GroupKind, number>();
  for (const c of calls) {
    const k = groupKindOf(c);
    by.set(k, (by.get(k) ?? 0) + 1);
  }
  // Map keeps the order each kind came; sort is stable
  return [...by].map(([kind, n]) => ({ kind, n })).sort((a, b) => b.n - a.n);
}

/** The folded line's icons, by kindsOf: other draws switch_mode's own glyph when every step it counts switched the
 * mode (the words still count it as other), and each case-law family draws the case-law glyph (case_law). */
export function iconsOf(calls: readonly ToolCallEntry[]): string[] {
  const modes = calls.every((c) => groupKindOf(c) !== "other" || c.kind === "switch_mode");
  return kindsOf(calls).map((k) =>
    k.kind === "other" && modes
      ? "switch_mode"
      : k.kind.startsWith("case_law_")
        ? "case_law"
        : k.kind,
  );
}

/** The most steps tried that a group with nothing finished names; the rest are "And 3 more.". */
const NAMED = 3;

/** The folded line for a group at rest. Its words and its icons count only the steps that finished, so a step still
 * waiting, or one that failed or was stopped, is never told as done. With none finished, the newest step still to
 * finish says what it is doing. With every step over and none finished, what was tried ("Tried to read Lease.pdf.",
 * up to three, the rest as more) is the trouble itself, said once and led by the break, never followed by "It failed.". */
export function restOf(
  lang: Lang,
  calls: readonly ToolCallEntry[],
): { kinds: string[]; line: string; trouble: string } {
  const done = calls.filter(finishedOf);
  if (done.length > 0)
    return { kinds: iconsOf(done), line: summaryOf(lang, done), trouble: troubleOf(lang, calls) };
  const waiting = [...calls].reverse().find(unsettled);
  if (waiting !== undefined)
    return {
      kinds: iconsOf([waiting]),
      line: doingOf(lang, waiting),
      trouble: troubleOf(lang, calls),
    };
  // a step whose state is not known (a newer Clauseo's) is neither done nor failed, nor running: with nothing tried
  // and not done, the line says the state isn't known, in no tense, with no break
  const tried = calls.filter((c) => failedOf(c) || c.status === "cancelled");
  const unread = calls.filter(unknownState);
  if (tried.length === 0 && unread.length > 0)
    return { kinds: iconsOf(unread), line: steps(lang, "State not known."), trouble: "" };
  // only pages that couldn't open or steps held back: said plainly, with no break
  const setbacks = calls.filter(setbackOf);
  if (tried.length === 0 && setbacks.length > 0)
    return { kinds: iconsOf(setbacks), line: setbackLine(lang, calls), trouble: "" };
  // each sentence once (D-077): two tasks with nothing to tell them apart read "Tried to run a task." once, the
  // other counted in the more
  const out: string[] = [];
  for (const c of tried) {
    const said = doneOf(lang, c);
    if (out.length < NAMED && !out.includes(said)) out.push(said);
  }
  const rest = tried.length - out.length;
  if (rest > 0) out.push(counted(lang, ["And 1 more.", "And {n} more."], rest));
  // the break leads what was tried, alone: a kind icon before it would mark the one step twice
  return { kinds: [], line: "", trouble: out.join(" ") };
}

/** What failed or was stopped in a group with a step that finished or is still to, in words for the folded line; ""
 * when nothing did. A group with every step over and none finished says what it tried instead (restOf). */
export function troubleOf(lang: Lang, calls: readonly ToolCallEntry[]): string {
  const failed = calls.filter(failedOf).length;
  const stopped = calls.filter((c) => c.status === "cancelled").length;
  const out: string[] = [];
  // counted on their own: the line before counts only what finished, so "of them" would point at nothing it named
  if (failed > 0) out.push(counted(lang, ["1 failed.", "{n} failed."], failed));
  if (stopped > 0) out.push(counted(lang, ["1 stopped.", "{n} stopped."], stopped));
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
  const n = calls.filter(finishedOf).length;
  return n === 0 ? "" : counted(lang, ["1 done so far.", "{n} done so far."], n);
}

/** The rows of Claude's own tools (tool-names.ts, ownToolOf): done, and tried. */
const OWN_DONE: Record<OwnAs, [done: StepKey, tried: StepKey]> = {
  plan: ["Planned the work.", "Tried to plan the work."],
  helper: ["Asked a helper.", "Tried to ask a helper."],
  stop: ["Stopped background work.", "Tried to stop background work."],
  ask: ["Asked you.", "Tried to ask you."],
};

/** A row's words: what it did once done ("Read Lease.pdf."), what it set out to do once it failed or was stopped
 * ("Tried to read Lease.pdf.", its mark saying how it ended), and what it is doing while it waits or runs. */
export function doneOf(lang: Lang, entry: ToolCallEntry): string {
  if (unsettled(entry)) return doingOf(lang, entry);
  if (unknownState(entry)) return namedOf(lang, entry);
  // a page that couldn't open says so, and why, with no Failed mark
  const miss = missOf(entry);
  if (miss !== null) return missWords(lang, entry, miss);
  const tried = entry.status !== "completed" || httpRefused(entry);
  const say = (done: StepKey, attempt: StepKey, vars?: Record<string, string>) =>
    steps(lang, tried ? attempt : done, vars);
  // a helper's call (helper-words.ts) says the task it was given; cut, the "…" ends the line without the full stop. With
  // no task it says a helper was asked
  // ChatGPT's wait for its helpers, and its other helper tools (helper-words.ts)
  if (helpersWaitOf(entry))
    return say("Waited for helpers.", "Tried to wait for the background work.");
  if (helperToolOf(entry)) return say("Worked with a helper.", "Tried to ask a helper.");
  if (helperOf(entry)) {
    const task = helperTaskOf(entry);
    // ChatGPT's spawn with no prompt worked with a helper; Claude's untitled Agent asked one
    if (task === "" && spawnOf(entry))
      return say("Worked with a helper.", "Tried to ask a helper.");
    if (task === "") return say("Asked a helper.", "Tried to ask a helper.");
    return say("Asked a helper: {task}.", "Tried to ask a helper: {task}.", { task }).replace(
      /…\.$/,
      "…",
    );
  }
  // Claude's Yield is a wait for its own background work, quietly said (tool-names.ts)
  if (yieldOf(entry))
    return say("Waited for the background work.", "Tried to wait for the background work.");
  // Claude's own tools say what they did, never their names (tool-names.ts)
  const own = ownToolOf(entry);
  if (own !== null) return say(...OWN_DONE[own]);
  // a skill by its name made readable, never its internal one (skill-words.ts)
  const guide = skillDone(lang, entry, tried);
  if (guide !== null) return guide;
  // a case-law tool says what it did, whatever kind the agent gave it (Claude's other, ChatGPT's execute)
  const law = caseLawDone(lang, entry.title, tried);
  if (law !== null) return law;
  // an MCP tool other than Clauseo's own, by its server and its name made readable, whatever kind the agent gave it
  const label = toolLabelOf(entry);
  if (mcpOf(entry.title, entry.raw_input?.inline) !== null) return usedDone(lang, label, tried);
  const p = placeOf(entry);
  const name = p.name;
  // the agent's own words where nothing else names it: as they are once done, after the attempt once not
  // a title that is a tool's name is never shown as it came: a known one says what it did; any other, under a paper's
  // kind, reads as the kind's line, else by its name made readable ("Used todo write.")
  const tool = toolNameOf(entry.title);
  const titled = (plain: string, paper = true) => {
    if (tool !== undefined)
      return tool.as !== null
        ? toolDone(lang, tool.as, tried)
        : paper
          ? plain
          : usedDone(lang, label, tried);
    const own = titleOf(entry.title);
    return own === "" ? plain : agentWords(own);
  };
  const agentWords = (own: string) =>
    tried
      ? steps(lang, "Tried: {title}.", { title: own.replace(/[.!?।…]+$/, "") })
      : sentenceOf(own);
  switch (kindOf(entry)) {
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
    case "fetch": {
      const sq = p.query === null ? null : siteQuery(p.query);
      if (sq?.site != null)
        return say("Searched {site} for '{query}'.", "Tried to search {site} for '{query}'.", {
          site: sq.site,
          query: sq.query,
        });
      return sq !== null
        ? say("Searched online for '{query}'.", "Tried to search online for '{query}'.", {
            query: sq.query,
          })
        : name !== null
          ? say("Read {name} online.", "Tried to read {name} online.", { name })
          : // ChatGPT's "Web search" is codex-acp's label, not the agent's words
            webSearchOf(entry)
            ? say("Looked online.", "Tried to look online.")
            : titled(say("Looked online.", "Tried to look online."));
    }
    case "execute": {
      // the agent's own words for the task, else the program and the paper it ran on (D-017)
      const t = taskOf(entry);
      // its own words after "Ran:"; cut, the "…" ends the line without the full stop
      if (t.own !== null)
        return say("Ran: {description}.", "Tried to run: {description}.", {
          description: afterRan(t.own),
        }).replace(/…\.$/, "…");
      // a tool's name the room doesn't know, by that name made readable
      if (tool?.as === null && t.program === null) return usedDone(lang, label, tried);
      if (t.program === null && t.described !== null)
        return say("Ran {description}.", "Tried to run {description}.", {
          description: t.described,
        });
      if (t.program === null)
        return tool?.as == null
          ? say("Ran a command.", "Tried to run a command.")
          : toolDone(lang, tool.as, tried);
      // a chained command is counted, not named by its first program ("Ran New-Item." for three); the fold under the
      // row still shows it whole
      if (t.count > 1)
        return say("Ran {n} commands.", "Tried to run {n} commands.", { n: String(t.count) });
      return t.paper === null
        ? say("Ran {program}.", "Tried to run {program}.", { program: t.program })
        : say("Ran {program} on {name}.", "Tried to run {program} on {name}.", {
            program: t.program,
            name: t.paper,
          });
    }
    case "think":
      return say("Thought it through.", "Tried to think it through.");
    default:
      // a step nothing names but its tool, by that tool's name made readable; with none, "Used a tool."
      return titled(usedDone(lang, label, tried), false);
  }
}

/** A row's hover: where the paper is, the folder within the matter or the place outside it; null with none. */
export function hoverOf(entry: ToolCallEntry): string | null {
  return placeOf(entry).where;
}

/** A row's state where it didn't simply finish. */
export function markOf(lang: Lang, call: ToolCallEntry): string | null {
  const held = heldMark(lang, call);
  if (held !== null) return held;
  if (missOf(call) !== null) return null;
  if (failedOf(call)) return th(lang, "Failed");
  if (call.status === "cancelled") return th(lang, "Stopped");
  if (call.background !== undefined) return mt(lang, "In the background");
  if (call.status === "pending") return steps(lang, "Waiting");
  // the mark, unless the row's words already say it
  if (unknownState(call))
    return namedOf(lang, call) === steps(lang, "State not known.")
      ? null
      : steps(lang, "State not known");
  return null;
}

/** The time a row shows: when it started (the host's first stamp), so the rows, in the order they started, read in
 * time order however long each took (D-020); none while it is pending or running,
 * since its state says enough. */
export function timeOf(call: ToolCallEntry): Timestamp | null {
  if (call.status === "pending" || call.status === "in_progress") return null;
  return call.started_at;
}

/** The group's line as StepGroup takes it: icons, words, the muted aside, the trouble, and live, what the step in hand
 * waits on. */
export type GroupLine = {
  kinds: string[];
  line: string;
  more: string;
  trouble: string;
  asks?: "permission" | "question";
};

/** The group's line: the summary, or live what is being done now with the count done so far. `asks`: the turn waits
 * on the lawyer (asksPick), so the step in hand is told as waiting for them ("Needs your permission."), never as
 * underway. */
export function lineOf(
  lang: Lang,
  entries: readonly (ViewEntry | undefined)[],
  live: boolean,
  asks: "permission" | "question" | null = null,
): GroupLine {
  // the agent's upkeep of its own notes (the matter brief) is no paper: it is neither counted nor told as failed
  const calls: ToolCallEntry[] = [];
  let upkeep = false;
  for (const e of entries)
    if (e?.tool_call !== undefined) {
      if (internalOf(e.tool_call)) upkeep = true;
      else calls.push(e.tool_call);
    }
  const aside = backgroundLine(lang, calls);
  // the pages that couldn't open and the steps held back, muted beside the line unless they are the line
  const setback = setbackLine(lang, calls);
  const beside = (line: string) =>
    [line === setback ? "" : setback, aside].filter((m) => m !== "").join(" ");
  // a group at rest of nothing but that upkeep says the work was planned, with no count and no break
  const rest = (): GroupLine => {
    if (calls.length === 0 && upkeep)
      return { kinds: ["think"], line: steps(lang, "Planned the work."), trouble: "", more: aside };
    const r = restOf(lang, calls);
    return { ...r, more: beside(r.line) };
  };
  if (!live) return rest();
  // the newest of what is still to finish, or a thought after the last of them
  let now: ToolCallEntry | "thought" | null = null;
  for (let i = entries.length - 1; i >= 0 && now === null; i -= 1) {
    const e = entries[i];
    if (e?.agent_text !== undefined) now = "thought";
    else if (e?.tool_call !== undefined && unsettled(e.tool_call)) now = e.tool_call;
    else if (e?.tool_call !== undefined) break;
  }
  if (now === null) return rest();
  return {
    // waiting on the lawyer, the line leads with the wait's own icon (the shield, the question mark), as the working
    // line does, never the kind of the step that waits
    ...(asks === null ? {} : { asks }),
    kinds: asks !== null ? [] : now === "thought" ? ["think"] : [now.kind as string],
    line:
      asks !== null
        ? asksWords(lang, asks)
        : now === "thought"
          ? stepLive(lang, "Thinking it through.")
          : doingOf(lang, now),
    more: [doneSoFar(lang, calls), beside("")].filter((m) => m !== "").join(" "),
    trouble: troubleOf(lang, calls),
  };
}
