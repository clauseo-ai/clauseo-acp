// A helper as the answer shows it (H1 of the helpers design, round 24): which of an answer's rows
// are the lead's and which a helper's, where a helper's card stands, what its card says. Pure;
// every view entry is read through helper-entries.ts. A helper's card stands where the step that
// launched it stood (C2's `call`; Claude's Agent step), else where its own entry arrived (ChatGPT
// has no launching step). Its steps and words leave the lead's step groups and show on its card. A
// step or words of a helper the window has no entry for stays flat, never dropped. A helper a
// helper started (ChatGPT nests) has its own small card inside its parent's.
import type { Lang } from "@clauseo/components/lib/display.ts";
import { steps } from "@clauseo/components/lib/words-steps.ts";
import { stepLive } from "@clauseo/components/lib/words-steps-live.ts";
import type { StreamView } from "../../store/view.ts";
import type { EntryId, TextOrBlob, ViewEntry } from "../../transport/protocol.ts";
import { pickables } from "../pickers.ts";
import { turnRunning } from "./live.ts";
import {
  HELPER_PREFIX,
  helperEntryId,
  helperPartOf,
  type HelperEntry,
  type HelperId,
} from "./helper-entries.ts";

/** How a helper stands. H1 draws these four; H3 adds its own (waiting for the lawyer, refused) to
 * this union. An `unknown` outcome reads as stopped. The host ends every helper (C2): failed when
 * its turn ends by itself, stopped on Stop or at start-up after a crash. A helper outlives its turn
 * only on Claude, carried into the turn of the lawyer's next message, and never past that turn. One
 * still open after that (`lapsed`: when its time ran out) reads as stopped, from a log that somehow
 * has one: no card counts minutes for an answer that is over. */
export type HelperState = "working" | "done" | "failed" | "stopped";

export function stateOf(h: HelperEntry, lapsed: number | undefined): HelperState {
  if (h.outcome === undefined) return lapsed === undefined ? "working" : "stopped";
  return h.outcome === "done" || h.outcome === "failed" ? h.outcome : "stopped";
}

export const settled = (s: HelperState): boolean => s !== "working";

const STEP = "step:";

const newestStepOf = new WeakMap<readonly EntryId[], EntryId | null>();
/** The newest turn's step entry id in the view, kept per id list; null with none. */
function newestStep(ids: readonly EntryId[]): EntryId | null {
  let found = newestStepOf.get(ids);
  if (found === undefined) {
    found = null;
    for (let i = ids.length - 1; i >= 0 && found === null; i -= 1)
      if (ids[i]!.startsWith(STEP)) found = ids[i]!;
    newestStepOf.set(ids, found);
  }
  return found;
}

/** When an open helper's time ran out: its turn's end (its step entry's time), unless that turn is
 * the newest to end and a turn runs now (Claude carried it into the lawyer's next message);
 * undefined while it may still work. */
export function lapsedOf(
  h: HelperEntry,
  v: StreamView | undefined,
  paged?: ReadonlyMap<EntryId, ViewEntry>,
): number | undefined {
  const id = `${STEP}${h.turn}` as EntryId;
  const ended = paged?.get(id)?.step ?? v?.entries.get(id)?.step;
  if (ended === undefined) return undefined;
  const carried = turnRunning(v?.header?.header?.status) && newestStep(v?.ids ?? []) === id;
  return carried ? undefined : ended.ts;
}

/** The answer's rows without what belongs to a helper whose entry the window has (`entryOf`: an
 * older page's or the view's), each card at its launching step's place when this answer has that
 * step, else at its own entry's. A helper a helper started stands inside its parent's card. */
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

/** A helper's card: its entry, its own steps and words in order (a step that launched a helper it
 * started left out), the entry ids of the helpers it started, and when its time ran out (lapsedOf).
 * A child's card (`fold`) takes the steps and words of every helper under it and draws no cards. */
export type HelperContent = {
  readonly helper: HelperEntry | undefined;
  readonly items: readonly (readonly [EntryId, ViewEntry])[];
  readonly children: readonly EntryId[];
  readonly lapsed: number | undefined;
};

const EMPTY: HelperContent = { helper: undefined, items: [], children: [], lapsed: undefined };

/** Of an older page's and the view's entries, in order: `helper`'s own steps and words, and the
 * helpers it started. Any page holding a helper's step also holds its entry (C2), so a card fills
 * in as older pages load. */
function membersOf(
  helper: HelperId,
  fold: boolean,
  all: readonly EntryId[],
  entryOf: (id: EntryId) => ViewEntry | undefined,
): { items: EntryId[]; children: EntryId[] } {
  const children: EntryId[] = [];
  const calls = new Set<string>();
  const parents = new Map<HelperId, HelperId | undefined>();
  for (const id of all) {
    const part = helperPartOf(entryOf(id));
    if (part?.kind !== "helper") continue;
    parents.set(part.entry.helper, part.entry.parent);
    if (!fold && part.entry.parent === helper) {
      children.push(id);
      if (part.entry.call !== undefined) calls.add(part.entry.call);
    }
  }
  // folded: a helper under this one, at any depth, is this one's
  const mine = (h: HelperId | null): boolean => {
    if (!fold) return h === helper;
    for (let at = h ?? undefined, n = 0; at !== undefined && n < 16; n += 1) {
      if (at === helper) return true;
      at = parents.get(at);
    }
    return false;
  };
  const items = all.filter((id) => {
    const e = entryOf(id);
    const part = helperPartOf(e);
    if (part?.kind === "text") return mine(part.entry.helper);
    if (part?.kind === "step")
      return mine(part.helper) && !calls.has(e?.tool_call?.tool_call ?? "");
    return false;
  });
  return { items, children };
}

/** A pick of one helper's card content over an older page (`paged`) and the view: the same object
 * while each part of it is the same. */
export function contentPick(
  helper: HelperId,
  paged: ReadonlyMap<EntryId, ViewEntry>,
  fold = false,
) {
  let ids: readonly EntryId[] | null = null;
  let members: { items: EntryId[]; children: EntryId[] } = { items: [], children: [] };
  let last = EMPTY;
  const self = helperEntryId(helper);
  return (v: StreamView | undefined): HelperContent => {
    const entryOf = (id: EntryId) => paged.get(id) ?? v?.entries.get(id);
    if (v === undefined || v.ids !== ids) {
      ids = v?.ids ?? null;
      members = membersOf(helper, fold, [...paged.keys(), ...(v?.ids ?? [])], entryOf);
    }
    const part = helperPartOf(entryOf(self));
    const entry = part?.kind === "helper" ? part.entry : undefined;
    const lapsed = entry === undefined ? undefined : lapsedOf(entry, v, paged);
    const items = members.items.flatMap((id) => {
      const e = entryOf(id);
      return e === undefined ? [] : [[id, e] as const];
    });
    const children = members.children;
    if (
      entry === last.helper &&
      lapsed === last.lapsed &&
      children.length === last.children.length &&
      children.every((c, i) => c === last.children[i]) &&
      items.length === last.items.length &&
      items.every((x, i) => x[1] === last.items[i]![1])
    )
      return last;
    last = { helper: entry, items, children, lapsed };
    return last;
  };
}

const helperIdsOf = new WeakMap<readonly EntryId[], readonly EntryId[]>();
/** The view's helper entry ids, kept per id list. */
export function helperIds(ids: readonly EntryId[]): readonly EntryId[] {
  let found = helperIdsOf.get(ids);
  if (found === undefined) {
    found = ids.filter((id) => id.startsWith(HELPER_PREFIX));
    helperIdsOf.set(ids, found);
  }
  return found;
}

/** How many top helpers still work in the running turn, those Claude carried into it included
 * (lapsedOf): the working line waits on them. One left open by an older turn doesn't count. */
export function workingHelpers(v: StreamView): number {
  let n = 0;
  for (const id of helperIds(v.ids)) {
    const part = helperPartOf(v.entries.get(id));
    if (part?.kind !== "helper") continue;
    const h = part.entry;
    if (h.parent !== undefined || h.outcome !== undefined) continue;
    if (lapsedOf(h, v) === undefined) n += 1;
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
export function stateWords(
  lang: Lang,
  h: HelperEntry,
  now: number,
  lapsed: number | undefined,
): string {
  const state = stateOf(h, lapsed);
  if (state === "working")
    return steps(lang, "Working for {time}", { time: minutesWords(lang, now - h.started_at) });
  const end = h.finished_at ?? lapsed;
  if (end === undefined) return "";
  const time = minutesWords(lang, end - h.started_at);
  if (state === "done") return steps(lang, "Took {time}", { time });
  if (state === "failed") return steps(lang, "Failed after {time}", { time });
  return steps(lang, "Stopped after {time}", { time });
}

/** The ms until the working line's minute next turns over, from when it started. */
export const untilNextMinute = (started: number, now: number): number =>
  60_000 - (Math.max(0, now - started) % 60_000);
