// Versions as the room reads them. The matter's history across its conversations, each version saying who made it,
// when and which papers it touched, and the whole matter as it stood after it: which papers were in the matter, and
// which revision each kept document stood at. Pure: no DOM, no React, and every history, paper name and date comes
// in as an argument. Restore and Undo always move forward and become versions of their own; the host makes those
// versions, and the parts only hand the lawyer's intent over.
import type { Lang } from "../lib/display.ts";
import { clock } from "../lib/words.ts";
import { listOf, vt } from "../lib/words-versions.ts";

/* ---------- days ---------- */

/** A calendar day, as an ISO date: "2026-10-14". */
export type Day = string;
/** "Wednesday 14 October" (long), "14 October" (date), "14 Oct" (short). */
export type DayForm = "long" | "date" | "short";
/** Which day is today, and the words for any day in either language. The app supplies both. */
export type Calendar = { today: Day; words: (lang: Lang, day: Day, form: DayForm) => string };

/** A day in words: "Today" for today, otherwise as the calendar says it. */
export function dayWords(lang: Lang, cal: Calendar, d: Day, form: DayForm): string {
  return d === cal.today ? vt(lang, "Today") : cal.words(lang, d, form);
}

/* ---------- the versions ---------- */

/** The whole matter after a version. */
export type MatterState = {
  /** The papers in the matter then, by id, in the wall's order. */
  papers: readonly string[];
  /** Each document whose words Versions keeps, by paper id: the revision it stood at, or null before it was made. */
  docs: Readonly<Record<string, string | null>>;
};

export type Who =
  /** Clauseo, in one of the matter's conversations, named as the lawyer named it; plan: the plan it asked with. */
  | { kind: "clauseo"; conversation: string; plan?: string }
  /** The lawyer, in Word or anywhere outside Clauseo. */
  | { kind: "outside" }
  /** An undo of the version with id `of`. Its time and day are read from that version when shown, so two versions
   * made in the same minute stay apart. */
  | { kind: "undo"; of: string }
  /** The papers first brought into the matter. */
  | { kind: "import"; n: number }
  /** A restore to the version with id `to`. */
  | { kind: "restore"; to: string }
  /** An undo of the restore made at `of`. */
  | { kind: "undoRestore"; of: string };

export type Version = {
  id: string;
  day: Day;
  /** The clock time, in English: "3:42 pm". */
  at: string;
  who: Who;
  /** The papers it changed, made and added, by id. */
  touched: readonly string[];
  added: readonly string[];
  made: readonly string[];
  state: MatterState;
  /** A file added with this version that is too large to keep versions of: its name and size, as the computer says. */
  large?: { file: string; size: string };
};

/** The time of the version that undoes, and its day. */
export type Stamp = { day: Day; at: string };

/** "3:42 pm" from minutes past midnight. */
export function minuteClock(m: number): string {
  const hour = Math.floor(m / 60) % 24;
  const min = m % 60;
  const h12 = hour % 12 === 0 ? 12 : hour % 12;
  return `${h12}:${String(min).padStart(2, "0")} ${hour < 12 ? "am" : "pm"}`;
}
/** Minutes past midnight from "3:42 pm". */
export function clockMinutes(at: string): number {
  const m = /^(\d+):(\d+) (am|pm)$/.exec(at);
  if (m === null) return 0;
  const hour = (Number(m[1]) % 12) + (m[3] === "pm" ? 12 : 0);
  return hour * 60 + Number(m[2]);
}

/* ---------- reading a version in words ---------- */

/** A paper's name, as its tile titles it, by id. */
export type NameOf = (id: string) => string;

export const timeOf = (lang: Lang, v: Version) => clock(lang, v.at);
export const dateOf = (lang: Lang, cal: Calendar, v: Version) => dayWords(lang, cal, v.day, "date");
export const isToday = (cal: Calendar, v: Version) => v.day === cal.today;
const when = (lang: Lang, cal: Calendar, v: Version) => ({
  time: timeOf(lang, v),
  date: dateOf(lang, cal, v),
});

/** "The matter as it stood at 3:42 pm on 14 October", or "... at 4:31 pm today". */
export function asItStood(lang: Lang, cal: Calendar, v: Version): string {
  return isToday(cal, v)
    ? vt(lang, "The matter as it stood at {time} today", { time: timeOf(lang, v) })
    : vt(lang, "The matter as it stood at {time} on {date}", when(lang, cal, v));
}

/** "3:42 pm on 14 October", or "4:31 pm today". */
export function whenOf(lang: Lang, cal: Calendar, v: Version): string {
  return isToday(cal, v)
    ? vt(lang, "{time} today", { time: timeOf(lang, v) })
    : vt(lang, "{time} on {date}", when(lang, cal, v));
}

/** The plan a conversation asked with, named on its versions muted and last. */
export const planOf = (v: Version): string | null =>
  v.who.kind === "clauseo" ? (v.who.plan ?? null) : null;

/** Who made it: "Clauseo, in Draft", "You, outside Clauseo", "Undo of the change at 4:05 pm". */
export function whoOf(lang: Lang, cal: Calendar, v: Version, all: readonly Version[]): string {
  const w = v.who;
  switch (w.kind) {
    case "clauseo":
      return vt(lang, "Clauseo, in {conversation}", { conversation: w.conversation });
    case "outside":
      return vt(lang, "You, outside Clauseo");
    case "undo": {
      const of = all.find((x) => x.id === w.of);
      if (of === undefined) return "";
      return of.day === v.day
        ? vt(lang, "Undo of the change at {time}", { time: timeOf(lang, of) })
        : vt(lang, "Undo of the change at {time} on {date}", when(lang, cal, of));
    }
    case "import":
      return vt(lang, "Imported {n} documents", { n: w.n });
    case "restore": {
      const to = all.find((x) => x.id === w.to);
      if (to === undefined) return "";
      return isToday(cal, to)
        ? vt(lang, "Restore to {time} today", { time: timeOf(lang, to) })
        : vt(lang, "Restore to {time} on {date}", when(lang, cal, to));
    }
    case "undoRestore":
      return vt(lang, "Undo of the restore at {time}", { time: clock(lang, w.of) });
  }
}

/** Ids in the order the version's matter holds them; any it doesn't hold go last. */
function inOrder(v: Version, ids: readonly string[]): string[] {
  const place = (id: string) => {
    const i = v.state.papers.indexOf(id);
    return i < 0 ? Number.MAX_SAFE_INTEGER : i;
  };
  return [...new Set(ids)].sort((a, b) => place(a) - place(b));
}

/** Which papers it touched, as one plain line: "Draft reply and List of dates". */
export function papersOf(lang: Lang, v: Version, name: NameOf): string {
  if (v.who.kind === "import") return "";
  return listOf(lang, inOrder(v, [...v.made, ...v.touched, ...v.added]).map(name));
}

/** What it did, in sentences: "Changed Junior's note. Added Letter from the bank." */
export function didOf(lang: Lang, v: Version, name: NameOf): string {
  const out: string[] = [];
  const names = (ids: readonly string[]) => listOf(lang, inOrder(v, ids).map(name));
  if (v.made.length > 0) out.push(vt(lang, "Made {papers}.", { papers: names(v.made) }));
  if (v.touched.length > 0) out.push(vt(lang, "Changed {papers}.", { papers: names(v.touched) }));
  if (v.added.length > 0 && v.who.kind !== "import")
    out.push(vt(lang, "Added {papers}.", { papers: names(v.added) }));
  if (v.large !== undefined) out.push(vt(lang, "Added {papers}.", { papers: v.large.file }));
  return out.join(" ");
}

/** The day a version belongs to, as a group key, and its heading. */
export const dayKey = (v: Version) => v.day;
export const dayHeading = (lang: Lang, cal: Calendar, v: Version) =>
  dayWords(lang, cal, v.day, "long");

/** Whether two versions were made by the same hand in the same place on the same day: one run on the timeline. */
export function sameHand(a: Version, b: Version): boolean {
  if (a.day !== b.day) return false;
  if (a.who.kind === "clauseo" && b.who.kind === "clauseo")
    return a.who.conversation === b.who.conversation;
  return a.who.kind === "outside" && b.who.kind === "outside";
}

/* ---------- what differs ---------- */

const revOf = (s: MatterState, d: string) => s.docs[d] ?? null;
const docsOf = (...states: MatterState[]) => [
  ...new Set(states.flatMap((s) => Object.keys(s.docs))),
];

/** The kept documents whose words differ between two states, in the order the states list them. */
export function differing(a: MatterState, b: MatterState): string[] {
  return docsOf(a, b).filter((d) => revOf(a, d) !== revOf(b, d));
}

/** The papers in now that then didn't have: added since. */
export const addedSince = (then: MatterState, now: MatterState): string[] =>
  now.papers.filter((p) => !then.papers.includes(p));

/** Each paper's place in the matter at a version: there, not there yet, or changed by this version. */
export type Era = "there" | "gone" | "touched";
export function eraOf(id: string, v: Version): Era {
  if (!v.state.papers.includes(id)) return "gone";
  if (
    v.touched.includes(id) ||
    v.made.includes(id) ||
    (v.added.includes(id) && v.who.kind !== "import")
  ) {
    return "touched";
  }
  return "there";
}

/** Whether a version changed or made any kept document, so its change can be undone on its own. */
export const changedDocs = (v: Version) => [...v.touched, ...v.made].some((d) => d in v.state.docs);

/* ---------- undoing, always forward ---------- */

const versionAt = (
  id: string,
  at: Stamp,
  who: Who,
  touched: readonly string[],
  state: MatterState,
): Version => ({
  id,
  day: at.day,
  at: at.at,
  who,
  touched,
  added: [],
  made: [],
  state,
});

/** now's documents, with those in `changed` taken from `from`. Papers added since stay. */
const mix = (now: MatterState, from: MatterState, changed: readonly string[]): MatterState => ({
  papers: now.papers,
  docs: Object.fromEntries(
    docsOf(now, from).map((d) => [d, changed.includes(d) ? revOf(from, d) : revOf(now, d)]),
  ),
});

/** The kept documents a version changed or made. */
const mine = (v: Version) =>
  docsOf(v.state).filter((d) => v.touched.includes(d) || v.made.includes(d));

/** The documents a version changed that a later version changed again, and the latest that did: undoing it asks
 * about each, Undo anyway or Keep current. */
export function changedSince(
  history: readonly Version[],
  id: string,
): { id: string; by: Version }[] {
  const i = history.findIndex((v) => v.id === id);
  const v = history[i];
  if (v === undefined) return [];
  const out: { id: string; by: Version }[] = [];
  for (const d of mine(v)) {
    let later: Version | undefined;
    for (const x of history.slice(i + 1)) if (revOf(x.state, d) !== revOf(v.state, d)) later = x;
    if (later !== undefined) out.push({ id: d, by: later });
  }
  return out;
}

/** Undoing a change moves forward: each document it changed goes back to how it stood just before it, unless the
 * lawyer keeps it as it is now. The undo is a version of its own. */
export function undoChange(
  history: readonly Version[],
  id: string,
  kept: readonly string[],
  at: Stamp,
): { version: Version; changed: string[] } | null {
  const i = history.findIndex((v) => v.id === id);
  const v = history[i];
  const before = history[i - 1];
  const now = history[history.length - 1];
  if (v === undefined || before === undefined || now === undefined) return null;
  const changed = mine(v).filter(
    (d) => !kept.includes(d) && revOf(now.state, d) !== revOf(before.state, d),
  );
  const who: Who = { kind: "undo", of: v.id };
  return {
    version: versionAt(
      `n${history.length + 1}`,
      at,
      who,
      changed,
      mix(now.state, before.state, changed),
    ),
    changed,
  };
}

/* ---------- the timeline: days, and runs by one hand ---------- */

export type Show = "all" | "clauseo" | "you";
/** How many days keep their versions in view before the rest fold under "Earlier". */
export const OPEN_DAYS = 4;
/** A run of this many versions or more by one hand folds behind its latest. */
export const FOLD_AT = 3;

/** A version and its place in history. */
export type Item = { v: Version; i: number };
export type Row =
  | { kind: "one"; item: Item }
  | { kind: "run"; key: string; head: Item; rest: Item[] };
export type Group = { key: string; head: string; count: number; rows: Row[] };

const byWhom = (show: Show, v: Version) =>
  show === "all" || (show === "clauseo") === (v.who.kind === "clauseo");
/** A version that made or added a paper stands on its own line, never folded into a run. */
const standsAlone = (v: Version) =>
  v.made.length > 0 || v.added.length > 0 || v.large !== undefined;
export const rowVersion = (r: Row) => (r.kind === "one" ? r.item.v : r.head.v);

/** Newest first: days, and in each day the runs by one hand. Now always stands on its own. */
export function groupsOf(
  history: readonly Version[],
  show: Show,
  lang: Lang,
  cal: Calendar,
): Group[] {
  const groups: Group[] = [];
  const last = history.length - 1;
  for (let i = last; i >= 0; i--) {
    const v = history[i];
    if (v === undefined || !byWhom(show, v)) continue;
    let g = groups[groups.length - 1];
    if (g === undefined || g.key !== dayKey(v)) {
      g = { key: dayKey(v), head: dayHeading(lang, cal, v), count: 0, rows: [] };
      groups.push(g);
    }
    g.count++;
    const prev = g.rows[g.rows.length - 1];
    const item = { v, i };
    const joins =
      prev !== undefined &&
      sameHand(rowVersion(prev), v) &&
      !standsAlone(v) &&
      !standsAlone(rowVersion(prev)) &&
      i !== last &&
      !(prev.kind === "one" && prev.item.i === last);
    // an earlier version of the same hand joins the run below the newest of it
    if (joins && prev.kind === "one") {
      g.rows[g.rows.length - 1] = {
        kind: "run",
        key: `run-${prev.item.v.id}`,
        head: prev.item,
        rest: [item],
      };
    } else if (joins && prev.kind === "run") prev.rest.push(item);
    else g.rows.push({ kind: "one", item });
  }
  // a run too short to fold stands as its versions
  for (const g of groups) {
    g.rows = g.rows.flatMap((r): Row[] =>
      r.kind === "run" && r.rest.length + 1 < FOLD_AT
        ? [{ kind: "one", item: r.head }, ...r.rest.map((item): Row => ({ kind: "one", item }))]
        : [r],
    );
  }
  return groups;
}

/** The days folded under "Earlier", as a span: "6 to 23 October", or "28 September to 3 October" across months. */
export function foldedSpan(lang: Lang, cal: Calendar, groups: readonly Group[]): string {
  const folded = groups.slice(OPEN_DAYS);
  const a = folded[folded.length - 1]?.key;
  const b = folded[0]?.key;
  if (a === undefined || b === undefined) return "";
  // within one month, the first day needs only its number
  const from =
    a.slice(0, 7) === b.slice(0, 7)
      ? String(Number(a.slice(8, 10)))
      : dayWords(lang, cal, a, "date");
  return vt(lang, "{from} to {to}", { from, to: dayWords(lang, cal, b, "date") });
}
