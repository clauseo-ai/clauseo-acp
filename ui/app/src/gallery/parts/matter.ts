// The parts gallery's invented matter: papers and judgments the parts name by id. None of it is real.
import type { Cite, CiteCheck, MatterData, Paper } from "@clauseo/components/clauseo/matter.ts";

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
    pb: "PB 1 to 4",
    kind: "correspondence",
    role: "supports",
    pin: "Para 2, PB 1",
    excerpt:
      "The bank was told in writing on the first of the month that the account was in dispute.",
    mark: "told in writing",
  },
  {
    id: "reply",
    title: "Reply to the notice, 20 March 2024",
    pb: "PB 5 to 7",
    kind: "correspondence",
    role: "against",
    pin: "Para 3, PB 5",
    excerpt: "The bank denies that any notice reached its branch before the account was closed.",
    mark: "denies",
  },
  {
    id: "agreement",
    title: "Supply agreement, 1 April 2022",
    pb: "PB 8 to 21",
    kind: "document",
    role: "relied",
    pin: "Clause 14, PB 17",
    excerpt: "Either party may end this agreement by thirty days' written notice to the other.",
    mark: "thirty days' written notice",
  },
  {
    id: "plaint",
    title: "Plaint, 2 May 2024",
    pb: "PB 22 to 40",
    kind: "pleading",
    role: null,
  },
  {
    id: "order",
    title: "Order of the trial court, 9 July 2024",
    pb: "PB 41 to 43",
    kind: "order",
    role: null,
  },
  {
    id: "note",
    title: "Note on limitation",
    pb: "Note",
    kind: "note",
    role: null,
  },
];

export const CITES: Readonly<Record<string, Cite>> = {
  acme: {
    short: "Acme Traders",
    name: "Acme Traders v. State",
    cite: "(2001) 1 SCC 1",
    decided: "12 January 2001",
    court: "Supreme Court",
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

export const MATTER: MatterData = { papers: PAPERS, cites: CITES };
