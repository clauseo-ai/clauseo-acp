// The parts gallery's invented matter: papers and judgments the parts name by id. None of it is real.
import type {
  Cite,
  CiteCheck,
  Citation,
  MatterData,
  Paper,
} from "@clauseo/components/clauseo/matter.ts";

const CHECK: CiteCheck = {
  waitingFor: "judgments since 2001 that cite it",
  result: "Good law",
  note: "Followed in later decisions; never doubted.",
  ms: 900,
};

export const PAPERS: readonly Paper[] = [
  {
    id: "notice",
    title: "Notice to the bank, 3 March 2024",
    pb: { from: 1, to: 4 },
    kind: "correspondence",
  },
  {
    id: "reply",
    title: "Reply to the notice, 20 March 2024",
    pb: { from: 5, to: 7 },
    kind: "correspondence",
  },
  {
    id: "agreement",
    title: "Supply agreement, 1 April 2022",
    pb: { from: 8, to: 21 },
    kind: "document",
  },
  { id: "plaint", title: "Plaint, 2 May 2024", pb: { from: 22, to: 40 }, kind: "pleading" },
  {
    id: "order",
    title: "Order of the trial court, 9 July 2024",
    pb: { from: 41, to: 43 },
    kind: "order",
  },
  { id: "note", title: "Note on limitation", pb: { word: "Note" }, kind: "note" },
];

/** The answer in the thread, and the shorter one beside the excerpt card, as the matter keys their citations. */
export const ANSWER_ID = "answer";
export const CARD_ANSWER_ID = "card-answer";

const NOTICE: Citation = {
  id: "notice-2",
  paperId: "notice",
  role: "supports",
  pin: "Para 2, PB 1",
  excerpt:
    "The bank was told in writing on the first of the month that the account was in dispute.",
  mark: "told in writing",
};
const AGREEMENT: Citation = {
  id: "agreement-14",
  paperId: "agreement",
  role: "relied",
  pin: "Clause 14, PB 17",
  excerpt: "Either party may end this agreement by thirty days' written notice to the other.",
  mark: "thirty days' written notice",
};

/** Each answer's citations of the papers. The thread's answer quotes two places in the notice. */
export const CITATIONS: Readonly<Record<string, readonly Citation[]>> = {
  [ANSWER_ID]: [
    NOTICE,
    {
      id: "notice-9",
      paperId: "notice",
      role: "supports",
      pin: "Para 9, PB 3",
      excerpt: "A copy of this notice is sent to your head office by registered post.",
      mark: "head office",
    },
    {
      id: "reply-3",
      paperId: "reply",
      role: "against",
      pin: "Para 3, PB 5",
      excerpt: "The bank denies that any notice reached its branch before the account was closed.",
      mark: "denies",
    },
    AGREEMENT,
  ],
  [CARD_ANSWER_ID]: [NOTICE, AGREEMENT],
};

export const CITES: Readonly<Record<string, Cite>> = {
  acme: {
    short: "Acme Traders",
    name: "Acme Traders v. State",
    cite: "(2001) 1 SCC 1",
    decided: "12 January 2001",
    court: "Supreme Court",
    forum: "court",
    bench: 3,
    pin: "para 17",
    stance: "for",
    check: CHECK,
  },
  bluebell: {
    short: "Bluebell Mills",
    name: "Bluebell Mills Ltd v. Union of India",
    cite: "(2004) 3 SCC 210",
    decided: "4 March 2004",
    court: "Supreme Court",
    forum: "court",
    bench: 2,
    pin: "para 9",
    stance: "for",
    check: { ...CHECK, waitingFor: "judgments since 2004 that cite it" },
  },
  coral: {
    short: "Coral Finance",
    name: "Coral Finance Co. v. Ravi Kumar",
    cite: "(2010) 2 ABC 45",
    decided: "18 August 2010",
    court: "High Court of Delhi",
    forum: "court",
    bench: 1,
    pin: null,
    stance: "against",
    check: {
      ...CHECK,
      result: "Distinguished",
      note: "Distinguished on its facts in a later case.",
    },
  },
};

export const MATTER: MatterData = {
  papers: PAPERS,
  cites: CITES,
  citations: CITATIONS,
  inView: ANSWER_ID,
};
