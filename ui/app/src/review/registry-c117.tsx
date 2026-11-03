// #117's copy review entries, the composer's: apart from registry.tsx, as registry-u4.tsx is. The `/` menu's name, the
// list's accessible name a screen reader says (it draws no heading), and the count its live region says as it opens
// (drawn nowhere either), over the room with a thread whose agent lists its commands, `/` typed in the focused field.
// The `@` menu's entries are the matter's papers: its name, its count, and the lines under its rows (loading, a list
// that couldn't be read, more matches than rows, a list that stops short), over the room with the matter, `@` typed.
// None of these words is in the design files: new strings, for the copy page.
import type { Entry } from "./entry.ts";
import { threadHost, type ThreadState } from "./hosts.ts";
import { RoomAsking } from "./states.tsx";
import { threadWords } from "./words.ts";
import type { PaperSummary } from "../transport/protocol.ts";
import { uuid } from "../testing/stream-script.ts";

/** The thread's header as Claude Code's agent fills it: a few of its commands, one taking an input. */
const COMMANDS = [
  { name: "compact", description: "Clear the conversation history but keep a summary in context" },
  { name: "init", description: "Initialize a new CLAUDE.md file with codebase documentation" },
  { name: "review", description: "Review a pull request", input_hint: "pull request number" },
];

/** The menu open over the agent's commands, or only the first of them. */
const open = (commands: typeof COMMANDS) => () => (
  <RoomAsking host={() => threadHost({ entries: [], commands })} question="/" send={false} />
);

/** A paper of the matter, at its path under the matter's folder. */
const paper = (path: string, n: number): PaperSummary => ({
  file: uuid(n, 8),
  path,
  size: 1000 + n * 37,
  modified: 1_730_000_000_000 + n * 3_600_000,
});

/** Three papers, as list_papers lists them (by path). */
const PAPERS: PaperSummary[] = [
  paper("papers/Lease deed.pdf", 1),
  paper("papers/Plaint.pdf", 2),
  paper("papers/Written statement.pdf", 3),
];

/** 120 papers, more than the menu's 50 rows: eight kinds of paper, each numbered, by path. */
const KINDS = [
  "Affidavit",
  "Bank statement",
  "Cheque copy",
  "Exhibit",
  "Notice",
  "Order",
  "Rent receipt",
  "Sale deed",
];
const MANY: PaperSummary[] = Array.from({ length: 120 }, (_, i) =>
  paper(`papers/${KINDS[i % KINDS.length]} ${Math.floor(i / KINDS.length) + 1}.pdf`, i + 1),
).sort((a, b) => (a.path < b.path ? -1 : a.path > b.path ? 1 : 0));

/** The menu open over the matter's papers, as H8 answers them, `@` typed in the focused field. */
const atOpen = (state: Omit<ThreadState, "entries">) => () => (
  <RoomAsking host={() => threadHost({ entries: [], ...state })} question="@" send={false} />
);

/** #117's entries, each id written as it is: an id never moves when an entry is added or taken out. */
export const C117: Entry[] = [
  {
    id: "C117-01",
    words: threadWords("Commands"),
    screen: "room/composer-menu",
    render: open(COMMANDS),
  },
  {
    id: "C117-02",
    words: threadWords("{n} commands"),
    screen: "room/composer-menu",
    render: open(COMMANDS),
  },
  {
    id: "C117-03",
    words: threadWords("1 command"),
    screen: "room/composer-menu",
    render: open(COMMANDS.slice(0, 1)),
  },
  {
    id: "C117-04",
    words: threadWords("Papers"),
    screen: "room/composer-menu",
    render: atOpen({ papers: PAPERS }),
  },
  {
    id: "C117-05",
    words: threadWords("1 paper"),
    screen: "room/composer-menu",
    render: atOpen({ papers: PAPERS.slice(0, 1) }),
  },
  {
    id: "C117-06",
    words: threadWords("{n} papers"),
    screen: "room/composer-menu",
    render: atOpen({ papers: PAPERS }),
  },
  {
    id: "C117-07",
    words: threadWords("Loading papers…"),
    screen: "room/composer-menu",
    render: atOpen({ papersAnswer: "pending" }),
  },
  {
    id: "C117-08",
    words: threadWords("Couldn't read this matter's papers."),
    screen: "room/composer-menu",
    render: atOpen({ papersAnswer: "failed" }),
  },
  {
    id: "C117-09",
    words: threadWords("First {shown} of {n}. Type to narrow."),
    screen: "room/composer-menu",
    render: atOpen({ papers: MANY }),
  },
  {
    id: "C117-10",
    words: threadWords("Not every file in this folder is listed."),
    screen: "room/composer-menu",
    render: atOpen({ papers: PAPERS, papersAnswer: "truncated" }),
  },
];
