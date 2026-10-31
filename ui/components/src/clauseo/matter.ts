// The matter as the room's parts read it: the papers and the judgments an answer cites, as data the app supplies,
// and pure helpers over them in either language. No DOM and no React, so the tests can hold them. The data reaches
// the parts through MatterContext (matter-context.ts).
import type { Lang } from "../lib/display.ts";
import { t, type Key } from "../lib/words.ts";

/** What a paper is. */
export type PaperKind = "pleading" | "correspondence" | "document" | "order" | "note";

/** Where a paper sits in the paper book: its pages, from and to (the same page twice for one page), or a word such
 * as "Note" in the numeral's place where it has no page. The word is the app's and shows as it is. */
export type PaperBookPlace =
  | { readonly from: number; readonly to: number }
  | { readonly word: string };

/** One paper of the matter: what the matter knows about it, whichever answer cites it. */
export type Paper = {
  readonly id: string;
  /** The title as filed, for example "Notice to the bank, 3 March 2024". Shown as it is, never translated. */
  readonly title: string;
  readonly pb: PaperBookPlace;
  readonly kind: PaperKind;
};
export type PaperId = Paper["id"];

/** How an answer uses a paper it cites. */
export type CitedRole = "supports" | "against" | "relied";

/** The id of an answer in the thread. */
export type AnswerId = string;

/** One answer's use of one place in a paper: its role, the place it pins and the words it quotes from there. An
 * answer that quotes two places in one paper holds two citations, each with its own id. */
export type Citation = {
  /** Its id within its answer; another answer's citation may use the same id. */
  readonly id: CitationId;
  readonly paperId: PaperId;
  readonly role: CitedRole;
  /** The pinned place the answer quotes: "Para 9, PB 4". */
  readonly pin: string;
  /** The words quoted from that place, verbatim. */
  readonly excerpt: string;
  /** The span of the excerpt the answer turns on, marked inside it; "" marks nothing. */
  readonly mark: string;
};
export type CitationId = string;
/** A citation as a reference names it: its answer, and its id there. */
export type CitationRef = { readonly answer: AnswerId; readonly id: CitationId };

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
  /** The day it was decided, as the app formats it in the lawyer's language ("1 January 2001" for an English
   * lawyer, a Hindi date for a Hindi one). The part shows it as given and never reads it as a date. */
  readonly decided: string;
  readonly court: string;
  /** Whether a court or a tribunal decided it. */
  readonly forum: Forum;
  /** How many judges sat. */
  readonly bench: number;
  /** The pinned paragraph, "para 17", or null. */
  readonly pin: string | null;
  /** Whether it is for the lawyer's side, or one the other side will cite. */
  readonly stance: "for" | "against";
  readonly check: CiteCheck;
};
export type CiteId = string;

/** The papers and judgments the parts can name, by id, and each answer's citations of the papers. */
export type MatterData = {
  readonly papers: readonly Paper[];
  readonly cites: Readonly<Record<CiteId, Cite>>;
  /** Each answer's citations, by the answer's id, in the order the answer cites them. A reference in an answer
   * looks only among its own answer's citations. */
  readonly citations: Readonly<Record<AnswerId, readonly Citation[]>>;
  /** The answer in view, whose citations light the wall; null before there is one. */
  readonly inView: AnswerId | null;
};

/** How the answer uses a paper. "accepted" is earlier work the lawyer accepted into the matter. */
export type PaperRole = CitedRole | "prepared" | "accepted";
/** A paper tile's whole state on the wall: uncited, or lit in its role. */
export type TileState = "uncited" | PaperRole;

export const paperIn = (matter: MatterData, id: string): Paper | undefined =>
  matter.papers.find((p) => p.id === id);
export const citeIn = (matter: MatterData, id: string): Cite | undefined =>
  Object.hasOwn(matter.cites, id) ? matter.cites[id] : undefined;

export const ROLE_WORD: Record<PaperRole, Key> = {
  supports: "Supports",
  against: "Against",
  relied: "Relied on",
  prepared: "Prepared",
  accepted: "Accepted",
};

/* ---------- the bench mark: one filled circle on a fixed scale ---------- */

/** Who decided a judgment: a court, or a tribunal. */
export type Forum = "court" | "tribunal";

export type Bench = "constitution" | "three" | "two" | "single" | "tribunal";
export const BENCH_LABEL: Record<Bench, Key> = {
  constitution: "Constitution bench",
  three: "Three judges",
  two: "Two judges",
  single: "Single judge",
  tribunal: "Tribunal",
};
/** 16 px for five or more judges, 12 for three or four, 8 for two, 6 for one, 4 for a tribunal. */
export function benchOf(forum: Forum, judges: number): Bench {
  if (forum === "tribunal") return "tribunal";
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

/** A paper book place in words, for labels: "PB 31", "PB 33 to 40", "PB 33 से 40". A word in the numeral's place,
 * such as "Note", comes as it is. */
export function pbWords(lang: Lang, pb: PaperBookPlace): string {
  if ("word" in pb) return pb.word;
  return pb.from === pb.to
    ? t(lang, "PB {n}", { n: pb.from })
    : t(lang, "PB {from} to {to}", { from: pb.from, to: pb.to });
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

/** A block's words and references: text, or a reference with its label. A paper's reference carries the id of the
 * answer's citation; a judgment's, the judgment's id. */
export type Segment =
  | { type: "text"; text: string }
  | { type: "ref"; kind: "paper" | "cite"; id: string; label: string };

/** A reference inside a block: [[p:id|label]] names one of the answer's citations of a paper, by the citation's id;
 * [[c:id]] a judgment by its short name. */
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

/** A citation, with the paper it names. */
export type CitedPaper = { readonly paper: Paper; readonly citation: Citation };

/** One answer's citations; none for an answer the matter has no citations for, or for no answer. */
export const citationsOf = (matter: MatterData, answer: AnswerId | null): readonly Citation[] =>
  answer !== null && Object.hasOwn(matter.citations, answer)
    ? (matter.citations[answer] ?? [])
    : [];

/** A citation by its id among one answer's citations, with the paper it names. Undefined when the answer has no
 * citation of that id, and when the citation names a paper the matter doesn't hold. Such a citation is bad data from
 * the host or the store: the parts drop it, so it lights nothing and opens no card, and whoever builds MatterData
 * (R12) checks for it and logs it, so the drop isn't the only place it shows. */
export function citedPaper(
  citations: readonly Citation[],
  papers: readonly Paper[],
  id: CitationId,
): CitedPaper | undefined {
  const citation = citations.find((c) => c.id === id);
  const paper = citation === undefined ? undefined : papers.find((p) => p.id === citation.paperId);
  return citation === undefined || paper === undefined ? undefined : { paper, citation };
}

/** Which role lights a paper the answer cites more than once in different roles: against over supports over relied
 * on, so a paper that tells against the lawyer anywhere in the answer never lights as support. */
const ROLE_ORDER: readonly CitedRole[] = ["against", "supports", "relied"];
const rank = (r: CitedRole) => ROLE_ORDER.indexOf(r);

/** The papers an answer cites, by the paper's id, in the order the answer first cites them, each with the citation
 * that lights its tile and opens from it: of its citations, the first in the strongest role (ROLE_ORDER). A citation
 * of a paper the matter doesn't hold is left out, as citedPaper leaves it out. */
export function citedPapers(
  citations: readonly Citation[],
  papers: readonly Paper[],
): Map<PaperId, CitedPaper> {
  const out = new Map<PaperId, CitedPaper>();
  for (const citation of citations) {
    const paper = papers.find((p) => p.id === citation.paperId);
    if (paper === undefined) continue;
    const held = out.get(paper.id);
    if (held === undefined || rank(citation.role) < rank(held.citation.role))
      out.set(paper.id, { paper, citation });
  }
  return out;
}

/** What a paper's card shows: the citation a reference named, or, from the paper's tile, the citation that lights it
 * for the answer in view. Undefined when there is none, or when the named citation is of another paper. */
export function citationFor(
  matter: MatterData,
  paperId: PaperId,
  named: CitationRef | undefined,
): CitedPaper | undefined {
  if (named === undefined)
    return citedPapers(citationsOf(matter, matter.inView), matter.papers).get(paperId);
  const found = citedPaper(citationsOf(matter, named.answer), matter.papers, named.id);
  return found?.paper.id === paperId ? found : undefined;
}

/* ---------- History ---------- */

/** One row of History: what happened, in the language, and its clock time. */
export type HistoryRow = { what: string; at: string };

/* ---------- script runs ---------- */

const DEVA = /[\u0900-\u097F]/;
const LATIN = /[A-Za-z]/;
/** Whether a string is wholly English, so its element takes lang="en" inside the Hindi interface. */
export const isLatinOnly = (s: string) => LATIN.test(s) && !DEVA.test(s);
