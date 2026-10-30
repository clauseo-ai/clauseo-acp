// The Versions samples' invented matter: a supply dispute with a bank, the papers and a draft reply, the versions made
// over ten days in late October, and the words of the draft and its list of dates at two times. None of it is real.
import type { Lang } from "@clauseo/components/lib/display.ts";
import type { PdfSide, SheetHeading } from "@clauseo/components/clauseo/version-page.tsx";
import {
  minuteClock,
  type Calendar,
  type MatterState,
  type Version,
  type Who,
} from "@clauseo/components/clauseo/versions-model.ts";

/** Today in the gallery's history: the last version is now. */
export const TODAY = "2026-10-29";

const LOCALE = { en: "en-GB", hi: "hi-IN" } as const;

/** The words for a day, as the page's language writes them. */
export const CAL: Calendar = {
  today: TODAY,
  words: (lang: Lang, day, form) =>
    new Intl.DateTimeFormat(LOCALE[lang], {
      timeZone: "UTC",
      day: "numeric",
      month: form === "short" ? "short" : "long",
      ...(form === "long" ? { weekday: "long" as const } : {}),
    }).format(new Date(`${day}T00:00:00Z`)),
};

/** Each paper's name, as its tile titles it, by id. */
const NAMES: Record<string, string> = {
  notice: "Notice to the bank",
  reply: "Reply to the notice",
  agreement: "Supply agreement",
  plaint: "Plaint",
  order: "Order of the trial court",
  note: "Note on limitation",
  dates: "List of dates",
  draft: "Written statement",
};
export const nameOf = (id: string) => NAMES[id] ?? id;

/** The papers in the wall's order: the matter's, then Clauseo's own. */
const ORDER = ["notice", "reply", "agreement", "plaint", "order", "note", "dates", "draft"];
const inOrder = (ids: readonly string[]) => ORDER.filter((id) => ids.includes(id));

const state = (
  papers: readonly string[],
  draft: string | null,
  dates: string | null,
): MatterState => ({
  papers: inOrder(papers),
  docs: { draft, dates },
});

const BASE = ["notice", "agreement", "plaint"];
const WITH_REPLY = [...BASE, "reply"];
const WITH_DATES = [...WITH_REPLY, "dates"];
const WITH_DRAFT = [...WITH_DATES, "draft"];
const WITH_ORDER = [...WITH_DRAFT, "order"];

const DRAFT: Who = { kind: "clauseo", conversation: "Draft", plan: "Quick" };
const DATES: Who = { kind: "clauseo", conversation: "Dates", plan: "Quick" };
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
  day,
  at,
  who,
  touched: [],
  added: [],
  made: [],
  state: s,
  ...o,
});

const OCT = (d: number) => `2026-10-${String(d).padStart(2, "0")}`;

/** The versions the story turns on, oldest first. */
const CORE: readonly Version[] = [
  v("v1", OCT(20), "11:02 am", { kind: "import", n: 3 }, state(BASE, null, null), {
    added: BASE,
  }),
  v("v2", OCT(21), "6:40 pm", YOU, state(WITH_REPLY, null, null), { added: ["reply"] }),
  v("v3", OCT(22), "3:58 pm", DATES, state(WITH_DATES, null, "D1"), { made: ["dates"] }),
  v("v4", OCT(23), "3:42 pm", DRAFT, state(WITH_DRAFT, "P1", "D1"), { made: ["draft"] }),
  v("v5", OCT(23), "4:05 pm", DRAFT, state(WITH_DRAFT, "P2", "D2"), {
    touched: ["dates", "draft"],
  }),
  v("v6", OCT(26), "11:20 am", { kind: "undo", of: "v5" }, state(WITH_DRAFT, "P1", "D1"), {
    touched: ["dates", "draft"],
  }),
  v("v7", OCT(27), "10:14 am", YOU, state(WITH_ORDER, "P1", "D1"), { added: ["order"] }),
  v("v8", TODAY, "4:31 pm", DRAFT, state(WITH_ORDER, "P3", "D3"), {
    touched: ["dates", "draft"],
  }),
  v("v9", TODAY, "4:58 pm", YOU, state(WITH_ORDER, "P4", "D3"), { touched: ["draft"] }),
];

/** A run of small versions by one hand: from minutes past midnight, so many minutes apart. */
type Burst = {
  anchor: string;
  side: "after" | "before";
  day: string;
  from: number;
  n: number;
  gap: number;
  who: Who;
  touched: readonly string[];
  large?: boolean;
};
const h = (hour: number, min: number) => hour * 60 + min;
export const LARGE = { file: "Bank statements, scanned.pdf", size: "740 MB" };

const BURSTS: readonly Burst[] = [
  {
    anchor: "v3",
    side: "after",
    day: OCT(22),
    from: h(16, 6),
    n: 5,
    gap: 4,
    who: DATES,
    touched: ["dates"],
  },
  {
    anchor: "v5",
    side: "after",
    day: OCT(23),
    from: h(16, 20),
    n: 4,
    gap: 6,
    who: DRAFT,
    touched: ["draft"],
  },
  {
    anchor: "v7",
    side: "after",
    day: OCT(27),
    from: h(15, 15),
    n: 2,
    gap: 31,
    who: YOU,
    touched: ["draft"],
  },
  {
    anchor: "v7",
    side: "after",
    day: OCT(27),
    from: h(16, 30),
    n: 4,
    gap: 5,
    who: DRAFT,
    touched: ["draft"],
  },
  {
    anchor: "v7",
    side: "after",
    day: OCT(28),
    from: h(9, 50),
    n: 1,
    gap: 1,
    who: YOU,
    touched: [],
    large: true,
  },
  {
    anchor: "v7",
    side: "after",
    day: OCT(28),
    from: h(11, 10),
    n: 5,
    gap: 6,
    who: DRAFT,
    touched: ["draft"],
  },
  {
    anchor: "v8",
    side: "before",
    day: TODAY,
    from: h(16, 12),
    n: 9,
    gap: 2,
    who: DRAFT,
    touched: ["dates", "draft"],
  },
];

/** Every version, oldest first: the core ones with the bursts laid in beside them. */
function lay(): Version[] {
  const out: Version[] = [];
  let k = 0;
  const small = (b: Burst, i: number, s: MatterState) =>
    v(`f${++k}`, b.day, minuteClock(b.from + i * b.gap), b.who, s, {
      touched: b.touched,
      ...(b.large === true ? { large: LARGE } : {}),
    });
  for (const c of CORE) {
    const prev = out[out.length - 1];
    for (const b of BURSTS)
      if (b.side === "before" && b.anchor === c.id && prev !== undefined)
        for (let i = 0; i < b.n; i++) out.push(small(b, i, prev.state));
    out.push(c);
    for (const b of BURSTS)
      if (b.side === "after" && b.anchor === c.id)
        for (let i = 0; i < b.n; i++) out.push(small(b, i, c.state));
  }
  return out;
}

/** The whole history, oldest first. */
export const HISTORY: readonly Version[] = lay();

/** The history as it stood after the draft's first version: four versions, no runs. */
export const EARLY: readonly Version[] = CORE.slice(0, 4);

export const at = (id: string): Version => {
  const found = HISTORY.find((x) => x.id === id);
  if (found === undefined) throw new Error(id);
  return found;
};

/** The version that brought in a large file, which its versions can't keep. */
export const LARGE_AT = HISTORY.find((x) => x.large !== undefined) ?? at("v9");

/** Clauseo's own papers at the wall's end. */
export const OWN = [
  { id: "dates", title: "List of dates", state: "accepted", word: "Dates" },
  { id: "draft", title: "Written statement", state: "prepared", word: "Draft" },
] as const;

/* ---------- the draft's paragraphs and the list of dates, by revision ---------- */

const p1 = "1. The plaintiff and the bank made a supply agreement on 1 April 2022.";
const p5 = "5. The bank therefore asks that the suit be dismissed.";

/** The paragraphs at each revision, by place: null where the paragraph isn't yet. */
export const DRAFT_REVS: Record<"P1" | "P4", readonly (string | null)[]> = {
  P1: [
    p1,
    "2. The plaint says the bank was told of the unpaid invoice.",
    "3. The agreement may end only on notice given as clause 14 requires.",
    "4. No notice was sent to the bank.",
    null,
    p5,
  ],
  P4: [
    p1,
    "2. The plaint says the bank was told of the unpaid invoice. It names no letter, date or officer.",
    "3. The agreement requires thirty days' written notice, sent by registered post, before it ends.",
    "4. The notice was sent to the bank by registered post on the same day{{, at Annexures P-2 and P-3}}.",
    "5. In Acme Traders v. State, (2001) 1 SCC 1, a notice given without that formality was held ineffective.",
    "6. The bank therefore asks that the suit{{, and the notice of 3 March 2024,}} be dismissed.",
  ],
};

/** The sheet's head, as filed. */
export const SHEET: SheetHeading = {
  court: "In the Court of the Civil Judge",
  caseLine: "Suit No. 14 of 2026",
  parties: [
    { name: "A. Rao", role: "Plaintiff" },
    { name: "Example Bank Ltd", role: "Defendant" },
  ],
  title: "Written statement",
};

export const ROWS = ["engaged", "meeting", "invoice", "return", "notice", "reply", "order"];
type Entry = readonly [string, string];
const D1: Record<string, Entry> = {
  invoice: ["12 February 2024", "The supplier sends its invoice."],
  return: ["20 February 2024", "The payment is returned."],
  notice: ["3 March 2024", "The defendant gives notice to the bank."],
  reply: ["20 March 2024", "The bank replies."],
  order: ["9 July 2024", "The trial court makes its order."],
};
/** The list of dates at each revision, by row. */
export const DATES_REVS: Record<"D1" | "D3", Readonly<Record<string, Entry>>> = {
  D1,
  D3: {
    ...D1,
    engaged: ["1 April 2022", "The parties sign the supply agreement."],
    meeting: ["9 January 2024", "The parties meet about the unpaid invoice."],
    return: ["21 February 2024", "The payment is returned, as the bank's memo records."],
  },
};

/** The PDF the note on limitation was saved as, then and now. */
export const PDF_THEN: PdfSide = {
  head: "As it was at 3:42 pm on 23 October",
  facts: "2 pages, 40 KB",
  saved: "Saved 23 October",
};
export const PDF_NOW: PdfSide = {
  head: "As it is now",
  facts: "3 pages, 52 KB",
  saved: "Saved 29 October",
};
export const PDF = {
  title: "Note on limitation",
  changed: "Changed since 3:42 pm on 23 October",
  unchanged: "Unchanged since 3:42 pm on 23 October",
};

/** The paper the first version did not hold yet, and when it came in. */
export const LATE = {
  title: "Reply to the notice",
  first: "Brought into the matter at 6:40 pm on 21 October.",
};

/** A version that couldn't be kept, at the top of the timeline. */
export const UNSAVED = {
  at: "5:10 pm",
  who: "Clauseo, in Draft",
  why: "Not kept as a version. Only 96 MB free on this computer.",
};
