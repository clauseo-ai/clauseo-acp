// The reader's invented data: a judgment the answer cites, the research's results, an entry of the CCI and NCLAT index,
// and a draft in review with its tracked changes and checked citations. The matter's names are the matter's; the words
// are invented.
import type {
  ChangeState,
  Draft,
  IndexEntry,
  Judgment,
  ReadAlready,
  Result,
  Review,
} from "@clauseo/components/clauseo/judgments.ts";

/** The clock time the samples use for what happened today. */
export const NOW = "4:46 pm";

/** A judgment the answer cites, as the reader holds it: the paragraphs the answer quotes, and the ones around them. */
export const JUDGMENT: Judgment = {
  id: "acme",
  short: "Acme Traders",
  name: "Acme Traders v. State",
  court: "Supreme Court",
  bench: 3,
  decided: "12 January 2001",
  stance: "for",
  reports: { scc: "(2001) 1 SCC 1", sccAt: "5", air: "AIR 2001 SC 14", airAt: "18" },
  parties: [
    { name: "Acme Traders", role: "Appellant" },
    { name: "State", role: "Respondent" },
  ],
  quoted: {
    para: 17,
    words: ["the notice must name the sum due", "a bare demand is not enough"],
  },
  paras: [
    {
      n: 16,
      text: "The appeal arises from a demand made by letter after the goods were delivered. The buyer paid part of the price and disputed the rest, and the seller treated the contract as ended.",
    },
    {
      n: 17,
      text: "We hold that the notice must name the sum due and say what is to be done about it, and that a bare demand is not enough to end a contract. A buyer who is not told the sum cannot pay it.",
    },
    {
      n: 18,
      text: "The letter here named no sum and gave no date. However firmly it was worded, it cannot be read as a notice under the contract.",
    },
    {
      n: 19,
      text: "The appeal is allowed in part. The matter goes back to the trial court to decide the sum due under the contract, on the evidence already on the record.",
    },
  ],
  citedBy: 1200,
  since: [
    {
      id: "bluebell",
      short: "Bluebell Mills",
      cite: "(2004) 3 SCC 210",
      decided: "4 March 2004",
      bench: 2,
      treatment: "followed",
      note: "Followed on the point that a notice must name the sum.",
    },
    {
      id: "hallam",
      short: "Hallam Traders",
      cite: "(2012) 6 SCC 90",
      decided: "9 May 2012",
      bench: 2,
      treatment: "followed",
      note: "Followed in a supply dispute with the same letter.",
    },
    {
      id: "coral",
      short: "Coral Finance",
      cite: "(2010) 2 ABC 45",
      decided: "18 August 2010",
      bench: 1,
      treatment: "distinguished",
      note: "Distinguished: the demand there named its sum.",
    },
  ],
  more: { followed: 40, distinguished: 6 },
  read: "10:12 am",
};

/** What Indian Kanoon returned for the research: the Supreme Court's judgments, each with its matched words. */
export const RESULTS: Result[] = [
  {
    id: "bluebell",
    short: "Bluebell Mills",
    name: "Bluebell Mills Ltd v. Union of India",
    cite: "(2004) 3 SCC 210",
    decided: "4 March 2004",
    bench: 2,
    stance: "for",
    fragment: "a letter that names **no sum** is not notice",
    read: "10:20 am",
  },
  {
    id: "coral",
    short: "Coral Finance",
    name: "Coral Finance Co. v. Ravi Kumar",
    cite: "(2010) 2 ABC 45",
    decided: "18 August 2010",
    bench: 1,
    stance: "against",
    fragment: "the demand there **named the sum** due",
    read: null,
  },
  {
    id: "hallam",
    short: "Hallam Traders",
    name: "Hallam Traders v. Board",
    cite: "(2012) 6 SCC 90",
    decided: "9 May 2012",
    bench: 2,
    stance: "for",
    fragment: "a **bare demand** does not stop the interest running",
    read: null,
  },
];

/** The judgments already read, which still open while Indian Kanoon is down. */
export const READ_ALREADY: ReadAlready[] = [
  { id: "acme", short: "Acme Traders", cite: "(2001) 1 SCC 1", read: "10:12 am" },
  { id: "bluebell", short: "Bluebell Mills", cite: "(2004) 3 SCC 210", read: "10:20 am" },
];

/** An entry of the CCI and NCLAT index, and the snapshot it comes from. */
export const INDEX_ENTRY: IndexEntry = {
  title: "Example Forum v. Sample Suppliers",
  rows: [
    { label: "Case", value: "CCI Case No. 14 of 2020" },
    { label: "Parties", value: "Sample Suppliers Pvt Ltd" },
    { label: "Under", value: "Section 4" },
    { label: "Order", value: "Order of 3 May 2024" },
    { label: "Outcome", value: "Closed, with a warning." },
  ],
};
export const SNAPSHOT = "1 June 2026";

/** A draft in review: the reply to the bank, with two changes Clauseo made, two the lawyer made in Word, and five
 * citations, each in a different state of its check. */
export const DRAFT: Draft = {
  file: "Reply to the bank.docx",
  title: "Reply to the bank",
  court: "Before the Example Tribunal",
  caseLine: "Application No. 1 of 2026",
  parties: [
    { name: "Harbour Foods Ltd", role: "Applicant" },
    { name: "Example Bank", role: "Respondent" },
  ],
  heading: "Reply",
  paras: [
    "1. The notice went to the bank on 3 March 2024.",
    "2. The bank replied{+c1: within a week+} and {-c2:refused-}{+c2:declined, relying on [[acme]]+}.",
    "{+y1:3. The reply cites [[bluebell]], [[coral]] and [[order]].+}",
    "4. The applicant asks that the demand be withdrawn{+y2:, following [[hallam]]+}.",
  ],
  cites: [
    { id: "acme", text: "Acme Traders, (2001) 1 SCC 1", status: "verified", by: "clauseo" },
    {
      id: "bluebell",
      text: "Bluebell Mills, (2004) 3 SCC 210",
      status: "mismatched",
      by: "you",
      mismatch: {
        draft: "any letter will do",
        para: 9,
        source: "A bare demand is not enough, and a letter that names no sum is not notice.",
      },
    },
    {
      id: "coral",
      text: "Coral Finance, (2010) 2 SCC 54",
      status: "notfound",
      by: "you",
      notFound: {
        cited: "(2010) 2 SCC 54",
        name: "Coral Finance Co. v. Ravi Kumar",
        right: "(2010) 2 ABC 45",
      },
    },
    {
      id: "order",
      text: "Order of the trial court, 9 July 2024",
      status: "unverifiable",
      by: "you",
    },
    {
      id: "hallam",
      text: "Hallam Traders, (2012) 6 SCC 90",
      status: "unchecked",
      by: "you",
      addedAt: "4:52 pm",
    },
  ],
  changes: [
    { id: "c1", by: "clauseo", at: "4:31 pm" },
    { id: "c2", by: "clauseo", at: "4:31 pm" },
    { id: "y1", by: "you", at: "4:47 pm" },
    { id: "y2", by: "you", at: "4:52 pm" },
  ],
  checkedAt: "4:49 pm",
  editsAt: "4:52 pm",
};

const PENDING: ChangeState = { state: "pending", at: null };

/** Every change still to decide, with the lawyer's edits in Word back in the matter. */
export const REVIEW_OPEN: Review = {
  changes: { c1: PENDING, c2: PENDING, y1: PENDING, y2: PENDING },
  edits: true,
  focus: null,
  statuses: {},
};

/** Two changes settled: one accepted, one rejected, and the lawyer's two still to decide. */
export const REVIEW_SETTLING: Review = {
  ...REVIEW_OPEN,
  changes: {
    c1: { state: "accepted", at: NOW },
    c2: { state: "rejected", at: NOW },
    y1: PENDING,
    y2: PENDING,
  },
};
