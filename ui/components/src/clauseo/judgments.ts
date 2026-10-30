// The judgments the reader opens, the results a research run returns, the citations in a draft with their checks, and
// the draft's tracked changes. Types and pure helpers only, no DOM and no data: the app supplies the judgments, and
// their own words are shown as given in both languages.

export type Stance = "for" | "against";
export type Treatment = "followed" | "distinguished" | "doubted";

/** One paragraph of a judgment, with its number. */
export type Para = { n: number; text: string };

/** A later judgment's treatment of this one. */
export type Since = {
  id: string;
  short: string;
  cite: string;
  decided: string;
  bench: number;
  treatment: Treatment;
  note: string;
};

export type Judgment = {
  id: string;
  short: string;
  name: string;
  court: "Supreme Court";
  bench: number;
  decided: string;
  stance: Stance;
  /** As the lawyer cites it, SCC first, then AIR: the report, and where the quoted paragraph falls in it. */
  reports: {
    scc: string | null;
    sccAt: string | null;
    air: string | null;
    airAt: string | null;
    other?: string;
  };
  parties: readonly [
    { name: string; role: "Appellant" | "Petitioner" },
    { name: string; role: "Respondents" | "Respondent" },
  ];
  /** The paragraph the answer quotes, and the words in it the answer relies on. */
  quoted: { para: number; words: readonly string[] };
  paras: readonly Para[];
  citedBy: number;
  since: readonly Since[];
  more: { followed: number; distinguished: number };
  /** When Clauseo read it from Indian Kanoon: then it opens again without a lookup, even while Indian Kanoon is down. */
  read: string | null;
};

/** "(2005) 8 SCC 89, at 103, para 19" and "AIR 2005 SC 3512, at 3518, para 19": SCC first, then AIR, and a judgment
 * not yet in either by the report it is in. */
export function citedAs(
  j: Judgment,
): { report: string; cite: string; at: string | null; para: number }[] {
  const out: { report: string; cite: string; at: string | null; para: number }[] = [];
  if (j.reports.scc !== null)
    out.push({ report: "SCC", cite: j.reports.scc, at: j.reports.sccAt, para: j.quoted.para });
  if (j.reports.air !== null)
    out.push({ report: "AIR", cite: j.reports.air, at: j.reports.airAt, para: j.quoted.para });
  if (j.reports.other !== undefined)
    out.push({
      report: j.reports.other.split(" ")[1] ?? "",
      cite: j.reports.other,
      at: null,
      para: j.quoted.para,
    });
  return out;
}

/** The first report a judgment is cited by, for its sheet's case line. */
export const firstReport = (j: Judgment) => j.reports.scc ?? j.reports.other ?? "";

/** A paragraph's words split around the words the answer relies on, so they can be marked. */
export function markRuns(
  text: string,
  words: readonly string[],
): { text: string; marked: boolean }[] {
  const out: { text: string; marked: boolean }[] = [];
  let rest = text;
  for (;;) {
    let at = -1;
    let hit = "";
    for (const w of words) {
      const i = rest.indexOf(w);
      if (i >= 0 && (at < 0 || i < at)) {
        at = i;
        hit = w;
      }
    }
    if (at < 0) break;
    if (at > 0) out.push({ text: rest.slice(0, at), marked: false });
    out.push({ text: hit, marked: true });
    rest = rest.slice(at + hit.length);
  }
  if (rest !== "") out.push({ text: rest, marked: false });
  return out;
}

/* ---------- what Indian Kanoon returned for the research ---------- */

export type Result = {
  id: string;
  short: string;
  name: string;
  cite: string;
  decided: string;
  bench: number;
  stance: Stance;
  /** Indian Kanoon's fragment, the matched words between ** and **. */
  fragment: string;
  /** When Clauseo read the whole judgment, or null if only its fragment was read. */
  read: string | null;
};

/** A judgment already read, which opens while Indian Kanoon is unavailable. */
export type ReadAlready = { id: string; short: string; cite: string; read: string };

/** The fields an entry of the CCI and NCLAT index carries, in the order it shows them. */
export type IndexLabel = "Case" | "Parties" | "Under" | "Order" | "Outcome" | "Appeal";
/** One entry of the CCI and NCLAT index, as its snapshot holds it, in the index's own English. */
export type IndexEntry = { title: string; rows: readonly { label: IndexLabel; value: string }[] };

/** A fragment split at its ** marks. */
export const fragmentRuns = (s: string) =>
  s.split("**").map((text, i) => ({ text, marked: i % 2 === 1 }));

/* ---------- the citations in the draft, and what the check found ---------- */

export type CheckStatus = "verified" | "unverifiable" | "mismatched" | "notfound" | "unchecked";

export type DraftCite = {
  id: string;
  /** As the draft writes it. */
  text: string;
  status: CheckStatus;
  /** Who put it in the draft: Clauseo, or the lawyer in Word. */
  by: "clauseo" | "you";
  /** For a mismatch: the draft's words and the judgment's paragraph. */
  mismatch?: { draft: string; para: number; source: string };
  /** For a citation not found: the report the draft gives, and the judgment's name and the report it is at. */
  notFound?: { cited: string; name: string; right: string };
  /** For a citation added after the last check: when it was added. */
  addedAt?: string;
};

/* ---------- the draft's tracked changes ---------- */

/** A tracked change's id, as the draft's markup names it. */
export type ChangeId = string;
export type Change = { id: ChangeId; by: "clauseo" | "you"; at: string };

/** A draft in review, as Word holds it: its file, its title and heading, its paragraphs in markup (see draftRuns),
 * the citations in it with their checks, and its tracked changes. */
export type Draft = {
  /** The Word file's name: "Notice to the bank.docx". */
  file: string;
  /** Its title, as the bar names it. */
  title: string;
  court: string;
  caseLine: string;
  parties: readonly { name: string; role: string }[];
  /** The heading on its first page. */
  heading: string;
  paras: readonly string[];
  cites: readonly DraftCite[];
  changes: readonly Change[];
  /** When its citations were last checked. */
  checkedAt: string;
  /** When the lawyer's own edits in Word came back, or null while they have not. */
  editsAt: string | null;
};

/** The draft's citation with this id. */
export const draftCiteIn = (draft: Draft, id: string): DraftCite | undefined =>
  draft.cites.find((c) => c.id === id);

/** One run of the draft: words that stand, words a change brought in or took out, or a citation. */
export type DraftRun =
  | { kind: "same"; text: string }
  | {
      kind: "ins" | "del";
      change: ChangeId;
      runs: readonly ({ kind: "text"; text: string } | { kind: "cite"; id: string })[];
    };

const inner = (s: string) =>
  s
    .split(/(\[\[\w+\]\])/)
    .filter((x) => x !== "")
    .map((x) => {
      const m = /^\[\[(\w+)\]\]$/.exec(x);
      return m?.[1] === undefined
        ? { kind: "text" as const, text: x }
        : { kind: "cite" as const, id: m[1] };
    });

/** A paragraph of the draft, read from its markup: {+id:words+} came in, {-id:words-} went out. */
export function draftRuns(para: string): (DraftRun | { kind: "cite"; id: string })[] {
  const out: (DraftRun | { kind: "cite"; id: string })[] = [];
  const re = /\{([+-])(\w+):([\s\S]*?)[+-]\}/g;
  let last = 0;
  for (let m = re.exec(para); m !== null; m = re.exec(para)) {
    if (m.index > last)
      for (const r of inner(para.slice(last, m.index)))
        out.push(r.kind === "text" ? { kind: "same", text: r.text } : r);
    out.push({ kind: m[1] === "+" ? "ins" : "del", change: m[2] ?? "", runs: inner(m[3] ?? "") });
    last = re.lastIndex;
  }
  if (last < para.length)
    for (const r of inner(para.slice(last)))
      out.push(r.kind === "text" ? { kind: "same", text: r.text } : r);
  return out;
}

/** Whether a whole paragraph came in with one change, so rejecting it takes out its number too. */
export const wholeParagraph = (para: string): ChangeId | null => {
  const m = /^\{\+(\w+):[\s\S]*\+\}$/.exec(para);
  return m === null ? null : (m[1] ?? null);
};

/** The draft's paragraphs as they stand: a whole paragraph whose change is gone is null, and the numbered paragraphs
 * after it close up, so "6. " after a dropped "5. " reads "5. ". */
export function standingParas(
  paras: readonly string[],
  gone: (change: ChangeId) => boolean,
): (string | null)[] {
  let shift = 0;
  return paras.map((para) => {
    const whole = wholeParagraph(para);
    if (whole !== null && gone(whole)) {
      if (/^\{\+\w+:\d+\. /.test(para)) shift += 1;
      return null;
    }
    if (shift === 0) return para;
    return para.replace(/^(\d+)\. /, (_m: string, n: string) => `${Number(n) - shift}. `);
  });
}

/* ---------- the draft in review ---------- */

export type ChangeState = { state: "pending" | "accepted" | "rejected"; at: string | null };
export type Review = {
  /** Every change's state, by id. */
  changes: Record<ChangeId, ChangeState>;
  /** Whether the lawyer's own edits in Word have come back into the matter. */
  edits: boolean;
  /** The change or citation the lawyer is looking at. */
  focus: string | null;
  /** A citation's status where it has moved on since the check, such as one checked again. */
  statuses: Partial<Record<string, CheckStatus | "checking">>;
};

/** The changes the draft holds now: Clauseo's, and the lawyer's once they have come back from Word. */
export const changesIn = (r: Review, changes: readonly Change[]): Change[] =>
  changes.filter((c) => r.edits || c.by === "clauseo");

/** The citations the draft holds now, on the same rule as its changes. */
export const citesIn = (r: Review, cites: readonly DraftCite[]): DraftCite[] =>
  cites.filter((c) => r.edits || c.by === "clauseo");

/** Minutes past midnight of a clock time such as "4:31 pm", for ordering; 0 when it has none. */
export function minutesOf(time: string): number {
  const m = /(\d+):(\d+)\s*(am|pm)?/i.exec(time);
  if (m === null) return 0;
  const h = (Number(m[1]) % 12) + (m[3]?.toLowerCase() === "pm" ? 12 : 0);
  return (m[3] === undefined ? Number(m[1]) : h) * 60 + Number(m[2]);
}
