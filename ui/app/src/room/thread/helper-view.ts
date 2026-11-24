// A helper as the answer shows it (H1 of the helpers design, round 24): which of an answer's rows
// are the lead's and which a helper's, where a helper's card stands, what its card says. Pure;
// every view entry is read through helper-entries.ts. A helper's card stands where the step that
// launched it stood (C2's `call`; Claude's Agent step), else where its own entry arrived (ChatGPT
// has no launching step). Its steps and words leave the lead's step groups and show on its card. A
// step or words of a helper the window has no entry for stays flat, never dropped. A helper a
// helper started (ChatGPT nests) belongs to its top helper's card until H3 draws it on its own.
import type { Lang } from "@clauseo/components/lib/display.ts";
import { steps } from "@clauseo/components/lib/words-steps.ts";
import { stepLive } from "@clauseo/components/lib/words-steps-live.ts";
import type { StreamView } from "../../store/view.ts";
import type { EntryId, TextOrBlob, ViewEntry } from "../../transport/protocol.ts";
import { pickables } from "../pickers.ts";
import { helperEntryId, helperPartOf, type HelperEntry, type HelperId } from "./helper-entries.ts";

/** How a helper stands. H1 draws these four; H3 adds its own (waiting for the lawyer, refused,
 * still working with no turn open after the lawyer's next message on Claude) to this union. An
 * `unknown` outcome reads as stopped. */
export type HelperState = "working" | "done" | "failed" | "stopped";

export const stateOf = (h: HelperEntry): HelperState =>
  h.outcome === undefined
    ? "working"
    : h.outcome === "done" || h.outcome === "failed"
      ? h.outcome
      : "stopped";

export const settled = (s: HelperState): boolean => s !== "working";

/** The answer's rows without what belongs to a helper whose entry the window has (`entryOf`: an
 * older page's or the view's), each card at its launching step's place when this answer has that
 * step, else at its own entry's. */
export function leadRows(
  ids: readonly EntryId[],
  entryOf: (id: EntryId) => ViewEntry | undefined,
): EntryId[] {
  const known = (h: HelperId) => helperPartOf(entryOf(helperEntryId(h)))?.kind === "helper";
  const here = new Set(ids);
  // a launching step's id, to the card that takes its place
  const at = new Map<EntryId, EntryId>();
  const placed = new Set<EntryId>();
  for (const id of ids) {
    const part = helperPartOf(entryOf(id));
    if (part?.kind !== "helper") continue;
    const h = part.entry;
    if (h.parent !== undefined && known(h.parent)) continue;
    const call = h.call === undefined ? null : (`tool_call:${h.call}` as EntryId);
    if (call !== null && here.has(call) && !at.has(call)) {
      at.set(call, id);
      placed.add(id);
    }
  }
  const out: EntryId[] = [];
  for (const id of ids) {
    const card = at.get(id);
    if (card !== undefined) {
      out.push(card);
      continue;
    }
    const part = helperPartOf(entryOf(id));
    if (part === null) out.push(id);
    else if (part.kind === "helper") {
      const h = part.entry;
      if (!placed.has(id) && !(h.parent !== undefined && known(h.parent))) out.push(id);
    } else if (part.kind === "text") {
      if (!known(part.entry.helper)) out.push(id);
    } else if (part.helper === null || !known(part.helper)) out.push(id);
  }
  return out;
}

/** A helper's card: its entry, and its steps and words in order, those of the helpers it started
 * among them. */
export type HelperContent = {
  readonly helper: HelperEntry | undefined;
  readonly items: readonly (readonly [EntryId, ViewEntry])[];
};

const EMPTY: HelperContent = { helper: undefined, items: [] };

/** The ids of an older page's and the view's entries that belong to `helper` or to a helper it
 * started, in order: any page holding a helper's step also holds its entry (C2), so a card fills in
 * as older pages load. */
function memberIds(
  helper: HelperId,
  all: readonly EntryId[],
  entryOf: (id: EntryId) => ViewEntry | undefined,
): EntryId[] {
  const parents = new Map<HelperId, HelperId | undefined>();
  for (const id of all) {
    const part = helperPartOf(entryOf(id));
    if (part?.kind === "helper") parents.set(part.entry.helper, part.entry.parent);
  }
  const mine = (h: HelperId | null): boolean => {
    for (let at: HelperId | undefined = h ?? undefined, n = 0; at !== undefined && n < 16; n += 1) {
      if (at === helper) return true;
      at = parents.get(at);
    }
    return false;
  };
  return all.filter((id) => {
    const part = helperPartOf(entryOf(id));
    if (part?.kind === "text") return mine(part.entry.helper);
    if (part?.kind === "step") return mine(part.helper);
    return false;
  });
}

/** A pick of one helper's card content over an older page (`paged`) and the view: the same object
 * while its entry and each of its items are the same. */
export function contentPick(helper: HelperId, paged: ReadonlyMap<EntryId, ViewEntry>) {
  let ids: readonly EntryId[] | null = null;
  let members: EntryId[] = [];
  let last = EMPTY;
  const self = helperEntryId(helper);
  return (v: StreamView | undefined): HelperContent => {
    const entryOf = (id: EntryId) => paged.get(id) ?? v?.entries.get(id);
    if (v === undefined || v.ids !== ids) {
      ids = v?.ids ?? null;
      members = memberIds(helper, [...paged.keys(), ...(v?.ids ?? [])], entryOf);
    }
    const part = helperPartOf(entryOf(self));
    const entry = part?.kind === "helper" ? part.entry : undefined;
    const items = members.flatMap((id) => {
      const e = entryOf(id);
      return e === undefined ? [] : [[id, e] as const];
    });
    if (
      entry === last.helper &&
      items.length === last.items.length &&
      items.every((x, i) => x[1] === last.items[i]![1])
    )
      return last;
    last = { helper: entry, items };
    return last;
  };
}

const helperIdsOf = new WeakMap<readonly EntryId[], readonly EntryId[]>();
/** The view's helper entry ids, kept per id list. */
export function helperIds(ids: readonly EntryId[]): readonly EntryId[] {
  let found = helperIdsOf.get(ids);
  if (found === undefined) {
    found = ids.filter((id) => id.startsWith("helper:"));
    helperIdsOf.set(ids, found);
  }
  return found;
}

/** How many of the view's top helpers still work: the working line waits on them. */
export function workingHelpers(v: StreamView): number {
  let n = 0;
  for (const id of helperIds(v.ids)) {
    const part = helperPartOf(v.entries.get(id));
    if (
      part?.kind === "helper" &&
      part.entry.parent === undefined &&
      part.entry.outcome === undefined
    )
      n += 1;
  }
  return n;
}

/** The working line while the lead waits on its helpers. */
export const waitingWords = (lang: Lang, n: number): string =>
  n === 1
    ? stepLive(lang, "Waiting for a helper to finish.")
    : stepLive(lang, "Waiting for {n} helpers to finish.", { n });

/** Names the app knows, in the lawyer's words (crates/profile/src/subagents.rs); "default" is no
 * name. */
const KNOWN = { "paper-reader": "Paper reader", researcher: "Researcher" } as const;

/** Whether a helper has a name to show: not blank, not "default". */
export const named = (name: string): boolean => {
  const n = name.trim().toLowerCase();
  return n !== "" && n !== "default";
};

/** A helper's name as the card says it: a known type in words, another as a sentence
 * ("case-finder": "Case finder"), and none as "Helper 2", by its place (`n`) among the
 * conversation's helpers without a name. */
export function nameWords(lang: Lang, name: string, n: number): string {
  if (!named(name)) return steps(lang, "Helper {n}", { n });
  const known = KNOWN[name.trim().toLowerCase() as keyof typeof KNOWN];
  if (known !== undefined) return steps(lang, known);
  const said = name.replace(/[-_]+/g, " ").trim();
  return said.charAt(0).toUpperCase() + said.slice(1);
}

/** The model in the picker's words: "claude-haiku-5-5" is "Haiku 5.5", "gpt-6.2-sol" is "6.2 Sol";
 * a bare alias capitalised; any other as it comes. */
export function modelWords(model: string | undefined): string | null {
  if (model === undefined || model.trim() === "") return null;
  const m = model.trim().toLowerCase();
  const up = (s: string) => s.charAt(0).toUpperCase() + s.slice(1);
  if (/^(haiku|sonnet|opus)$/.test(m)) return up(m);
  const claude = /^claude-([a-z]+)-(\d+)-(\d+)/.exec(m);
  if (claude !== null) return `${up(claude[1]!)} ${claude[2]}.${claude[3]}`;
  const gpt = /^gpt-([\d.]+)(?:-([a-z]+))?$/.exec(m);
  if (gpt !== null) return gpt[2] === undefined ? gpt[1]! : `${gpt[1]} ${up(gpt[2])}`;
  return model.trim();
}

/** A pick of the name the conversation's model menu gives `model` (C2 sends the id the menu lists,
 * such as "claude-haiku-5-5"); null when the menu doesn't list it, and modelWords says it. */
export const menuNamePick =
  (model: string | undefined) =>
  (v: StreamView | undefined): string | null => {
    if (model === undefined) return null;
    const menu = pickables(v?.header?.header?.config_options).find((p) => p.category === "model");
    const name = menu?.choices.find((c) => c.value === model)?.name.trim();
    return name === undefined || name === "" ? null : name;
  };

/** Words cut to `max` characters at a word, with "…"; whole when short. One line: whitespace one
 * space. */
export function cut(text: string, max: number): string {
  const line = text.replace(/\s+/g, " ").trim();
  const chars = Array.from(line);
  if (chars.length <= max) return line;
  const head = chars.slice(0, max - 1).join("");
  const at = head.lastIndexOf(" ");
  return `${(at > max * 0.6 ? head.slice(0, at) : head).trimEnd()}…`;
}

/** A text the card can show without loading a blob: the inline one, else null. */
export const inlineOf = (t: TextOrBlob | undefined): string | null => t?.inline ?? null;

/** How long, in whole minutes: "less than a minute", "1 min", "4 min". No seconds, no ticking. */
export function minutesWords(lang: Lang, ms: number): string {
  const m = Math.floor(Math.max(0, ms) / 60_000);
  return m === 0 ? steps(lang, "less than a minute") : steps(lang, "{n} min", { n: m });
}

/** The card's state on the right: "Working for 3 min" (now: the clock the card last read), "Took 4
 * min", "Failed after less than a minute", "Stopped after 2 min"; "" for an ended helper with no
 * end time. */
export function stateWords(lang: Lang, h: HelperEntry, now: number): string {
  const state = stateOf(h);
  if (state === "working")
    return steps(lang, "Working for {time}", { time: minutesWords(lang, now - h.started_at) });
  if (h.finished_at === undefined) return "";
  const time = minutesWords(lang, h.finished_at - h.started_at);
  if (state === "done") return steps(lang, "Took {time}", { time });
  if (state === "failed") return steps(lang, "Failed after {time}", { time });
  return steps(lang, "Stopped after {time}", { time });
}

/** The ms until the working line's minute next turns over, from when it started. */
export const untilNextMinute = (started: number, now: number): number =>
  60_000 - (Math.max(0, now - started) % 60_000);
