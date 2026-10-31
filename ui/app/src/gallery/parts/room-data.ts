// The room's invented data, in the props the room's parts take: one question and its answer, the work behind it, the
// rail's matters and their conversations, the draft and its page. The matter's papers and judgments are in matter.ts.
import type { WorkItem } from "@clauseo/components/clauseo/answer.tsx";
import type { ConversationRowProps } from "@clauseo/components/clauseo/rail-conversations.tsx";
import type { FindableRow, MatterRowProps, RailGroup } from "@clauseo/components/clauseo/rail.tsx";
import type { Block, Cite, HistoryRow } from "@clauseo/components/clauseo/matter.ts";
import type { PreparedTile } from "@clauseo/components/clauseo/paper-tile.tsx";
import type { ReadyHint } from "@clauseo/components/clauseo/status-line.tsx";
import { MATTER } from "./matter.ts";

/** A handler the sample does nothing with. */
export const nothing = (): void => undefined;

/** The matter's head: its name as filed, and the line under it. */
export const MATTER_HEAD = {
  title: "Acme Traders v. State",
  titleHint: "District Court, Example. CS 12 of 2024. For Acme Traders",
  nextDate: "9 November 2026",
  caseNo: "CS 12 of 2024",
  court: "District Court, Example",
};

/** The plan's meter in the toolbar: plenty of it left, and close to the limit. */
export const PLAN_PLENTY = {
  name: "Claude",
  left: 0.6,
  valueText: "60% left",
  status: "Plenty left until the 9:40 pm reset",
};
export const PLAN_CLOSE = {
  name: "Claude",
  left: 0.2,
  valueText: "20% left",
  status: "Close to the limit. Resets at 9:40 pm",
};

/** History, newest first: the actor first, each row with its time. */
export const HISTORY: readonly HistoryRow[] = [
  { what: "You asked: Did the bank know about the notice?", at: "4:31 pm" },
  { what: "Clauseo answered, with 4 judgments checked", at: "4:31 pm" },
  { what: "You accepted the list of dates", at: "4:02 pm" },
  { what: "Clauseo added the reply to the papers", at: "3:40 pm" },
  { what: "You added 14 papers", at: "3:12 pm" },
];

/** The question in the thread, and what Clauseo did for its answer. */
export const QUESTION = {
  who: "You",
  time: "4:31 pm",
  text: "Did the bank know about the notice before it closed the account?",
};
export const ANSWER_WHO = { who: "Clauseo", time: "4:31 pm" };
export const ANSWER_DONE = "Checked 4 judgments. 14 papers read.";
export const WORK: readonly WorkItem[] = [
  { at: "4:29 pm", what: "Read the notice, PB 1 to 4" },
  { at: "4:29 pm", what: "Read the reply, PB 5 to 7" },
  { at: "4:30 pm", what: "Searched the supply agreement for notice terms" },
  { at: "4:31 pm", what: "Checked Acme Traders v. State against later decisions" },
];

/** The answer in blocks: a lead, two paragraphs and the other side's best case. Refs are [[p:citation|label]] or [[c:id]]. */
export const ANSWER: readonly Block[] = [
  {
    kind: "lead",
    text: "The bank was told in writing on the first of the month [[p:notice-2|PB 1]], before it closed the account, and a copy went to its head office [[p:notice-9|PB 3]].",
  },
  {
    kind: "para",
    text: "The reply denies that any notice reached the branch [[p:reply-3|PB 5]]. The supply agreement, though, needs only written notice to the other party [[p:agreement-14|Clause 14, PB 17]].",
  },
  {
    kind: "para",
    text: "[[c:acme]] holds that written notice to the head office counts as notice to the bank.",
  },
  {
    kind: "otherside",
    label: "The other side's best case",
    text: "They will cite [[c:coral]]: a branch's own rules decide where notice must go.",
  },
];
export const ANSWER_COUNT = "Cites 1 judgment";

/** A shorter answer for the excerpt card: its two references open the paper's words and the judgment's check. */
export const CARD_ANSWER: readonly Block[] = [
  {
    kind: "lead",
    text: "The bank was told in writing [[p:notice-2|PB 1]], and the supply agreement sets the notice period [[p:agreement-14|Clause 14]].",
  },
  {
    kind: "para",
    text: "[[c:bluebell]] supports reading a notice sent to head office as notice to the bank.",
  },
];

/** The accept row's words. */
export const ACCEPT_WORDS = {
  action: "Accept into the matter",
  pending: "Not yet accepted",
  done: "Accepted 4:33 pm",
  undo: "Undo",
};

/** The judgments the answer cites, in the order the wall lists them, and whether each check has finished. */
export const CITE_ROWS: readonly { id: string; cite: Cite; checked: boolean }[] = (
  [
    ["acme", true],
    ["bluebell", false],
    ["coral", true],
  ] as const
).flatMap(([id, checked]) => {
  const cite = MATTER.cites[id];
  return cite === undefined ? [] : [{ id, cite, checked }];
});

/** The draft's tile on the wall, and its ready card. */
export const DRAFT_TILE: PreparedTile = {
  id: "draft",
  short: "Draft reply",
  title: "Draft reply to the notice",
  label: "Prepared, not opened",
};
export const DRAFT = {
  title: "Draft reply to the notice",
  where: "Acme matter folder",
  why: "Answers the bank's denial",
  pages: 9,
  unopened: "Not opened",
};
export const DRAFT_PREVIEW = {
  court: "District Court, Example",
  caseLine: "Civil suit No. 12 of 2024",
  parties: [
    { name: "Acme Traders", role: "Plaintiff" },
    { name: "Example Bank", role: "Defendant" },
  ],
  title: "Reply to the notice of 3 March 2024",
  paras: [
    "The defendant received written notice of the plaintiff's claim on the first of the month.",
    "The defendant's branch did not act on it before the account was closed.",
  ],
};

/** The questions the matter offers over an empty field. */
export const PROMPTS = [
  "When was the notice sent?",
  "What does the agreement say about ending it?",
];

/** The ready hint at the status line, for a draft nobody has opened. */
export const READY_HINT: ReadyHint = {
  what: "Draft reply ready",
  label: "Draft reply, ready and not yet opened",
  state: "ready",
  onTurn: nothing,
};

/** Who answers: the plan a question goes to. */
export const PLANS = [
  { value: "claude", label: "Claude" },
  { value: "chatgpt", label: "ChatGPT" },
];

/** The rail. The current matter stands in its date group, with its conversations under it. */
const CONVERSATIONS: readonly ConversationRowProps[] = [
  {
    id: "c1",
    title: "Limitation on the notice",
    agent: "Claude",
    current: true,
    other: { agent: "ChatGPT", onContinue: nothing },
  },
  { id: "c2", title: "Draft reply to the bank", agent: "Claude", state: "question" },
  { id: "c3", title: "Dates in the notice", agent: "ChatGPT", state: "ready", at: "3:58 pm" },
];

export const PINNED: readonly MatterRowProps[] = [
  {
    name: "Supply agreement dispute",
    note: "Commercial suit, for the buyer",
    hint: "Supply agreement dispute",
    pinned: true,
    onOpen: nothing,
  },
];

export const RAIL_GROUPS: readonly RailGroup[] = [
  {
    when: "Today",
    matters: [
      {
        name: "Acme Traders v. State",
        note: "Recovery suit, for the plaintiff",
        hint: "Acme Traders v. State",
        current: true,
        state: "working",
        at: "4:32 pm",
        conversations: CONVERSATIONS,
        onNewConversation: nothing,
      },
      {
        name: "Harbour Trust",
        note: "Trust deed",
        hint: "Harbour Trust",
        state: "paused",
        at: "6:10 pm",
        onOpen: nothing,
      },
    ],
  },
  {
    when: "Tomorrow",
    matters: [
      {
        name: "Bluebell Mills v. Union of India",
        note: "Customs writ",
        hint: "Bluebell Mills",
        state: "pausedSet",
        at: "6:10 pm",
        onOpen: nothing,
      },
    ],
  },
  {
    when: "No date",
    matters: [
      {
        name: "Coral Finance v. Kumar",
        note: "Loan recovery, for the lender",
        hint: "Coral Finance",
        state: "ready",
        onOpen: nothing,
      },
    ],
  },
];

/** Every matter, with its client and case number, for finding one by name, client or number. */
export const FIND_ALL: readonly FindableRow[] = [
  {
    name: "Acme Traders v. State",
    note: "Recovery suit, for the plaintiff",
    hint: "Acme Traders v. State",
    client: "Acme Traders",
    caseNo: "CS 12 of 2024",
    onOpen: nothing,
  },
  {
    name: "Harbour Trust",
    note: "Trust deed",
    hint: "Harbour Trust",
    client: "A. Rao",
    caseNo: "OS 40 of 2023",
    onOpen: nothing,
  },
  {
    name: "Coral Finance v. Kumar",
    note: "Loan recovery, for the lender",
    hint: "Coral Finance",
    client: "Coral Finance Co.",
    caseNo: "CS 3 of 2025",
    onOpen: nothing,
  },
];
/** The matters opened most recently, newest first. */
export const FIND_RECENT: readonly MatterRowProps[] = [
  {
    name: "Acme Traders v. State",
    note: "Recovery suit, for the plaintiff",
    hint: "Acme Traders v. State",
    onOpen: nothing,
  },
  { name: "Harbour Trust", note: "Trust deed", hint: "Harbour Trust", onOpen: nothing },
];

/** The rail's notice about Clauseo itself: an update, waiting until this answer is done. */
export const NOTICE = {
  title: "An update is ready",
  line: "Restarts after this answer, by 4:40 pm.",
};

/** The working line's examples. */
export const CHECKING = {
  name: "Acme Traders v. State",
  waitingFor: "judgments since 2001 that cite it",
  expected: "4:40 pm",
};

/** Asking before a change: the paper and the papers Clauseo changed. */
export const PERMISSION_PAPER = "Notice to the bank";
export const CHANGED = ["Notice to the bank.docx", "Draft reply"];

/** An accepted earlier exchange: its question, its reply and what it made. */
export const EARLIER = {
  at: "3:58 pm",
  question: "Make a list of the dates in the notice and the reply",
  reply: "Here is a list of the dates in the papers, each with its page.",
  made: {
    title: "List of dates",
    entries: "7 entries",
    acceptedAt: "4:02 pm",
    accepted: "Accepted 4:02 pm",
  },
};

/** The empty states' counts and the search that finds nothing. */
export const PAPER_COUNT = 14;
export const MISSING_QUERY = "Tulip";

/** Where Settings is linked from the rail's foot. */
export const SETTINGS_LINK = "#settings";

/** A search for a client's name, which finds a matter by its client. */
export const RAIL_QUERY = "Rao";
