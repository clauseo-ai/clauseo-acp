// The matter as the room's parts read it: the papers and the judgments an answer cites, as data the app supplies,
// and pure helpers over them in either language. No DOM and no React, so the tests can hold them. The data reaches
// the parts through MatterContext (matter-context.ts).
import type { Lang } from "../lib/display.ts";
import { t, type Key } from "../lib/words.ts";

/** What a paper is. */
export type PaperKind = "pleading" | "correspondence" | "document" | "order" | "note";

/** One paper of the matter, as its tile and its excerpt card show it. */
export type Paper = {
  readonly id: string;
  /** The title as filed, for example "Notice to the bank, 3 March 2024". Shown as it is, never translated. */
  readonly title: string;
  /** Where it sits in the paper book: "PB 13", "PB 1 to 12", or a word such as "Note" where it has no page. */
  readonly pb: string;
  readonly kind: PaperKind;
  /** How the answer uses it, or null when the answer doesn't cite it. */
  readonly role: "supports" | "against" | "relied" | null;
  /** The pinned place the answer quotes: "Para 9, PB 4". */
  readonly pin?: string;
  /** The words quoted from that place, verbatim. */
  readonly excerpt?: string;
  /** The span of the excerpt the answer turns on, marked inside it. */
  readonly mark?: string;
};
export type PaperId = Paper["id"];

/** A judgment's check against later decisions, as the answer reports it. */
export type CiteCheck = {
  /** What the check is waiting on: "judgments since 2001 that cite it". */
  readonly waitingFor: string;
  /** Its result: "Good law". */
  readonly result: string;
  /** One line on how the judgment holds up. */
  readonly note: string;
  /** How long the check took, in milliseconds. */
  readonly ms: number;
};

/** A judgment the answer cites. */
export type Cite = {
  /** The name the answer uses: "Acme Traders". */
  readonly short: string;
  /** The full cause title. */
  readonly name: string;
  /** The report: "(2001) 1 SCC 1". */
  readonly cite: string;
  /** The date it was decided, in words: "1 January 2001". */
  readonly decided: string;
  readonly court: string;
  /** How many judges sat. */
  readonly bench: number;
  /** The pinned paragraph, "para 17", or null. */
  readonly pin: string | null;
  /** Whether it is for the lawyer's side, or one the other side will cite. */
  readonly stance: "for" | "against";
  readonly check: CiteCheck;
};
export type CiteId = string;

/** The papers and judgments the parts can name, by id. */
export type MatterData = {
  readonly papers: readonly Paper[];
  readonly cites: Readonly<Record<CiteId, Cite>>;
};

/** How the answer uses a paper. "accepted" is earlier work the lawyer accepted into the matter. */
export type PaperRole = "supports" | "against" | "relied" | "prepared" | "accepted";
/** A paper tile's whole state on the wall: uncited, or lit in its role. */
export type TileState = "uncited" | PaperRole;

export const paperIn = (matter: MatterData, id: string): Paper | undefined =>
  matter.papers.find((p) => p.id === id);
export const citeIn = (matter: MatterData, id: string): Cite | undefined =>
  Object.hasOwn(matter.cites, id) ? matter.cites[id] : undefined;

/** A cited paper's role in the answer. */
export const roleOf = (p: Paper): PaperRole | null => p.role;

export const ROLE_WORD: Record<PaperRole, Key> = {
  supports: "Supports",
  against: "Against",
  relied: "Relied on",
  prepared: "Prepared",
  accepted: "Accepted",
};

/* ---------- the bench mark: one filled circle on a fixed scale ---------- */

export type Bench = "constitution" | "three" | "two" | "single" | "tribunal";
export const BENCH_LABEL: Record<Bench, Key> = {
  constitution: "Constitution bench",
  three: "Three judges",
  two: "Two judges",
  single: "Single judge",
  tribunal: "Tribunal",
};
/** 16 px for five or more judges, 12 for three or four, 8 for two, 6 for one, 4 for a tribunal. */
export function benchOf(court: string, judges: number): Bench {
  if (court === "Tribunal") return "tribunal";
  if (judges >= 5) return "constitution";
  if (judges >= 3) return "three";
  if (judges === 2) return "two";
  if (judges === 1) return "single";
  return "tribunal";
}
/** "three judges" in an English sentence, "तीन न्यायाधीश" in Hindi. */
export const benchWords = (lang: Lang, b: Bench) =>
  lang === "hi" ? t(lang, BENCH_LABEL[b]) : BENCH_LABEL[b].toLowerCase();

/* ---------- words that must hold together ---------- */

const NB = "\u00A0";
/** No-break spaces inside "8 SCC 89", "para 40", "PB 31", and a word joiner after the hyphen in "Form DIR-12". */
export const keepIds = (s: string) =>
  s
    .replace(/\bForm DIR-(\d+)/g, `Form${NB}DIR-\u2060$1`)
    .replace(/\((\d{4})\) (\d+) SCC (\d+)/g, `($1)${NB}$2${NB}SCC${NB}$3`)
    .replace(/\b(\d+) SCC (\d+)/g, `$1${NB}SCC${NB}$2`)
    .replace(/LiveLaw \(SC\) (\d+)/g, `LiveLaw${NB}(SC)${NB}$1`)
    .replace(/\b(para|Para|paragraph|Paragraph|PB|Item|section|Step) (\d+)/g, `$1${NB}$2`)
    .replace(/(पैरा|मद|धारा|संख्या|पृष्ठ) (\d+)/g, `$1${NB}$2`);

/* ---------- paper book pages ---------- */

/** "PB 33 to 40" -> { from: "33", to: "40" }; "Note" -> a word in the numeral's place. */
export function pages(
  pb: string,
): { kind: "pages"; from: string; to: string | null } | { kind: "word"; word: string } {
  const m = /^PB (\d+)(?: to (\d+))?$/.exec(pb);
  if (m === null || m[1] === undefined) return { kind: "word", word: pb };
  return { kind: "pages", from: m[1], to: m[2] ?? null };
}
/** A paper book page in words, for labels: "PB 33 to 40", "PB 33 से 40". A word in the numeral's place, such as
 * "Note", is the app's and comes as it is. */
export function pbWords(lang: Lang, pb: string): string {
  const p = pages(pb);
  if (p.kind === "word") return pb;
  return p.to === null
    ? t(lang, "PB {n}", { n: p.from })
    : t(lang, "PB {from} to {to}", { from: p.from, to: p.to });
}
/** A paper's short title: cut before the first comma. */
export const shortTitle = (title: string) => title.split(",")[0] ?? title;
/** A tile has room for one short line: the words before " to ", where the title has them. */
export const tileTitle = (title: string) => shortTitle(title).split(" to ")[0] ?? title;

/* ---------- an answer's blocks ---------- */

/** One block of the answer: kind is "lead", "para" or "otherside". */
export type Block = { readonly kind: string; readonly text: string; readonly label?: string };
/** The same words without their sentence end, for a row of labels. */
export const bare = (s: string) => s.replace(/[.\u0964]$/, "");
/** A sentence's end in its language: the full stop, or the danda. */
export const sentence = (lang: Lang, s: string) =>
  s.replace(/[.\u0964]?$/, lang === "hi" ? "\u0964" : ".");

/** A block's words and references: text, or a reference to a paper or a judgment with its label. */
export type Segment =
  | { type: "text"; text: string }
  | { type: "ref"; kind: "paper" | "cite"; id: string; label: string };

/** A reference inside a block: [[p:id|label]] names a paper, [[c:id]] a judgment by its short name. */
const REF = /\[\[([pc]):([\w-]+)(?:\|([^\]]+))?\]\]/g;

/** A block's text split into words and references. A judgment's reference without its own label takes its short
 * name from the matter; any other reference without one shows its id. */
export function segmentsOf(block: string, matter: MatterData): Segment[] {
  const out: Segment[] = [];
  let last = 0;
  for (const m of block.matchAll(REF)) {
    if (m.index > last) out.push({ type: "text", text: block.slice(last, m.index) });
    const kind = m[1] === "p" ? "paper" : "cite";
    const id = m[2] ?? "";
    const named = kind === "cite" ? citeIn(matter, id)?.short : undefined;
    out.push({ type: "ref", kind, id, label: m[3] ?? named ?? id });
    last = m.index + m[0].length;
  }
  if (last < block.length) out.push({ type: "text", text: block.slice(last) });
  return out;
}

/** The papers an answer cites, by id, with their role. */
export function citedPapers(matter: MatterData): Map<string, PaperRole> {
  const out = new Map<string, PaperRole>();
  for (const p of matter.papers) {
    const r = roleOf(p);
    if (r !== null) out.set(p.id, r);
  }
  return out;
}

/* ---------- History ---------- */

/** One row of History: what happened, in the language, and its clock time. */
export type HistoryRow = { what: string; at: string };

/* ---------- script runs ---------- */

const DEVA = /[\u0900-\u097F]/;
const LATIN = /[A-Za-z]/;
/** Whether a string is wholly English, so its element takes lang="en" inside the Hindi interface. */
export const isLatinOnly = (s: string) => LATIN.test(s) && !DEVA.test(s);
