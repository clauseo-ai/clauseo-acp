// An invented matter's history, for the tests of Versions: four papers brought in, a list of dates and a draft made
// in two conversations, the lawyer's edits in Word, an undo, and runs of small versions between them across twelve
// days of March 2027. Neutral data, nothing from a real matter.
import type { Lang } from "../lib/display.ts";
import {
  minuteClock,
  type Calendar,
  type MatterState,
  type Version,
  type Who,
} from "./versions-model.ts";

export const TODAY = "2027-03-29";

const MONTH = [
  "January",
  "February",
  "March",
  "April",
  "May",
  "June",
  "July",
  "August",
  "September",
  "October",
  "November",
  "December",
];
const WEEKDAY = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];

/** The app's words for a day; the fixture says them in English in both languages, as the app may pass any. */
export const CAL: Calendar = {
  today: TODAY,
  words: (_lang: Lang, day, form) => {
    const d = new Date(`${day}T00:00:00Z`);
    const month = MONTH[d.getUTCMonth()] ?? "";
    const date = `${d.getUTCDate()} ${form === "short" ? month.slice(0, 3) : month}`;
    return form === "long" ? `${WEEKDAY[d.getUTCDay()] ?? ""} ${date}` : date;
  },
};

/** Each paper's name, as its tile titles it. */
export const NAMES: Record<string, string> = {
  plaint: "Plaint",
  notice: "Notice to the bank",
  reply: "Reply from the bank",
  letter: "Letter from the bank",
  receipt: "Postal receipt",
  memo: "Junior's note",
  dates: "List of dates",
  draft: "Draft reply",
};
export const nameOf = (id: string) => NAMES[id] ?? id;

const FIRST = ["plaint", "notice", "receipt", "memo"];
const WALL = ["plaint", "notice", "reply", "letter", "receipt", "memo", "dates", "draft"];
const order = (ids: readonly string[]) => WALL.filter((id) => ids.includes(id));
const WITH_REPLY = order([...FIRST, "reply"]);
const WITH_DATES = order([...WITH_REPLY, "dates"]);
const WITH_DRAFT = order([...WITH_DATES, "draft"]);
const WITH_LETTER = order([...WITH_DRAFT, "letter"]);

const state = (
  papers: readonly string[],
  draft: string | null,
  dates: string | null,
  memo: string,
): MatterState => ({
  papers,
  docs: { draft, dates, memo },
});

const DRAFT: Who = { kind: "clauseo", conversation: "Draft", plan: "Claude" };
const LIST: Who = { kind: "clauseo", conversation: "Dates", plan: "Claude" };
const YOU: Who = { kind: "outside" };
const v = (
  id: string,
  day: string,
  at: string,
  who: Who,
  s: MatterState,
  o: Partial<Version> = {},
): Version => ({
  id,
  day: `2027-03-${day}`,
  at,
  who,
  touched: [],
  added: [],
  made: [],
  state: s,
  ...o,
});

/** The versions the story turns on, oldest first. */
export const CORE: readonly Version[] = [
  v("v1", "01", "11:02 am", { kind: "import", n: 4 }, state(FIRST, null, null, "M1"), {
    added: FIRST,
  }),
  v("v2", "03", "6:40 pm", YOU, state(WITH_REPLY, null, null, "M1"), { added: ["reply"] }),
  v("v3", "07", "3:58 pm", LIST, state(WITH_DATES, null, "D1", "M1"), { made: ["dates"] }),
  v("v4", "09", "3:42 pm", DRAFT, state(WITH_DRAFT, "P1", "D1", "M1"), { made: ["draft"] }),
  v("v5", "09", "4:05 pm", DRAFT, state(WITH_DRAFT, "P2", "D2", "M1"), {
    touched: ["dates", "draft"],
  }),
  v(
    "v6",
    "11",
    "11:20 am",
    { kind: "undo", of: "4:05 pm", ofDay: "2027-03-09" },
    state(WITH_DRAFT, "P1", "D1", "M1"),
    {
      touched: ["dates", "draft"],
    },
  ),
  v("v7", "15", "10:14 am", YOU, state(WITH_LETTER, "P1", "D1", "M2"), {
    touched: ["memo"],
    added: ["letter"],
  }),
  v("v8", "29", "4:31 pm", DRAFT, state(WITH_LETTER, "P3", "D3", "M2"), {
    touched: ["dates", "draft"],
  }),
  v("v9", "29", "4:58 pm", YOU, state(WITH_LETTER, "P4", "D3", "M2"), { touched: ["draft"] }),
];

/** A run of small versions on a day: from minutes past midnight, so many, so many minutes apart, by one hand. */
type Run = {
  day: string;
  from: number;
  n: number;
  gap: number;
  who: Who;
  touched: readonly string[];
  large?: true;
} & ({ before: string } | { after: string });
const h = (hour: number, min: number) => hour * 60 + min;
export const LARGE = { file: "Bank statements, scanned.pdf", size: "740 MB" };

const RUNS: readonly Run[] = [
  { after: "v3", day: "07", from: h(16, 6), n: 5, gap: 4, who: LIST, touched: ["dates"] },
  { after: "v5", day: "10", from: h(10, 20), n: 6, gap: 7, who: DRAFT, touched: ["draft"] },
  { after: "v7", day: "20", from: h(11, 2), n: 4, gap: 9, who: DRAFT, touched: ["draft"] },
  { after: "v7", day: "26", from: h(10, 5), n: 3, gap: 9, who: DRAFT, touched: ["draft"] },
  { after: "v7", day: "27", from: h(15, 15), n: 2, gap: 31, who: YOU, touched: ["draft"] },
  { after: "v7", day: "27", from: h(16, 30), n: 4, gap: 5, who: DRAFT, touched: ["draft"] },
  { after: "v7", day: "28", from: h(9, 50), n: 1, gap: 1, who: YOU, touched: [], large: true },
  { after: "v7", day: "28", from: h(11, 10), n: 5, gap: 6, who: DRAFT, touched: ["draft"] },
  {
    before: "v8",
    day: "29",
    from: h(16, 12),
    n: 9,
    gap: 2,
    who: DRAFT,
    touched: ["dates", "draft"],
  },
];

/** Every version, oldest first: the core ones with the runs laid in between. */
function lay(): Version[] {
  const out: Version[] = [];
  let k = 0;
  const small = (r: Run, i: number, s: MatterState) =>
    v(`f${++k}`, r.day, minuteClock(r.from + i * r.gap), r.who, s, {
      touched: r.touched,
      ...(r.large === true ? { large: LARGE } : {}),
    });
  for (const c of CORE) {
    const prev = out[out.length - 1];
    for (const r of RUNS) {
      if ("before" in r && r.before === c.id && prev !== undefined)
        for (let i = 0; i < r.n; i++) out.push(small(r, i, prev.state));
    }
    out.push(c);
    for (const r of RUNS)
      if ("after" in r && r.after === c.id)
        for (let i = 0; i < r.n; i++) out.push(small(r, i, c.state));
  }
  return out;
}

export const HISTORY: readonly Version[] = lay();

export const at = (id: string): Version => {
  const found = HISTORY.find((x) => x.id === id);
  if (found === undefined) throw new Error(id);
  return found;
};

/* ---------- the draft's paragraphs and the list of dates, by revision ---------- */

export const PARA = {
  p1: "1. The defendant is a customer of the bank and holds a current account at its main branch.",
  p2a: "2. The plaint says the defendant was told of the charge on the account.",
  p2b: "2. The plaint says the defendant was told of the charge on the account. It names no letter, date or officer.",
  p3a: "3. A bank may levy a charge only on notice given in the manner the account terms require.",
  p3c: "3. The account terms require notice in writing, sent by registered post, at least thirty days before a charge.",
  p4a: "4. The notice was not sent to the defendant.",
  p4b: "4. The notice was sent to the bank by registered post on the same day.",
  p4c: "4. The notice was sent to the bank by registered post on the same day{{, at Annexures P-2 and P-3}}.",
  p5b: "5. In Acme Ltd v. State, (2001) 1 ABC 1, a charge levied without such notice was set aside.",
  p6a: "5. The defendant therefore asks that the suit be dismissed.",
  p6c: "6. The defendant therefore asks that the suit{{, and the charge of 2 March 2027,}} be dismissed.",
  hi6b: "6. अतः वादी प्रार्थना करता है कि नोटिस को रद्द किया जाए।",
  hi6c: "6. अतः वादी प्रार्थना करता है कि नोटिस{{ और 2 मार्च 2027 का आदेश}} को रद्द किया जाए।",
} as const;

export const DRAFT_REVS: Record<string, readonly (string | null)[]> = {
  P1: [PARA.p1, PARA.p2a, PARA.p3a, PARA.p4a, null, PARA.p6a],
  P4: [PARA.p1, PARA.p2b, PARA.p3c, PARA.p4c, PARA.p5b, PARA.p6c],
};

export const ROWS = ["engaged", "meeting", "invoice", "return", "notice", "reply", "order"];
type Entry = readonly [string, string];
const D1: Record<string, Entry> = {
  invoice: ["12 June 2026", "The bank sends its invoice."],
  return: ["20 June 2026", "The payment is returned."],
  notice: ["7 July 2026", "The defendant sends the notice."],
  reply: ["2 August 2026", "The bank replies."],
  order: ["14 November 2026", "The court issues summons."],
};
export const DATES_REVS: Record<string, Readonly<Record<string, Entry>>> = {
  D1,
  D3: {
    ...D1,
    engaged: ["1 April 2025", "The defendant opens the account."],
    meeting: ["3 March 2026", "The bank's officers meet the defendant."],
    return: ["21 June 2026", "The payment is returned, as the bank's memo records."],
  },
};
