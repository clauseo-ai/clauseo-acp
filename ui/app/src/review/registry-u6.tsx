// U6's copy review entries: apart from registry.tsx, as registry-u4.tsx is. Retrying from a question, each string
// over the parts that show it, on their own, inside the room's providers: the room doesn't yet make a retry (H13
// lands later), so no room state shows these. A new conversation's line and the status line's plan, in the room over
// a host with H8's papers and no threads yet, so the matter opens on a new conversation.
import { QuestionLine } from "@clauseo/components/clauseo/answer.tsx";
import { RoomScopeProvider } from "@clauseo/components/clauseo/room-scope-provider.tsx";
import { PartsRoot } from "@clauseo/components/ui/root.tsx";
import { titleOf } from "../room/conversation-title.ts";
import { RetriedLine } from "../room/retried-line.tsx";
import { RetryFromHere } from "../room/retry-from-here.tsx";
import { ThreadActionsContext, type ThreadActions } from "../room/thread/session.ts";
import type { Entry } from "./entry.ts";
import type { PaperSummary } from "../room/host-next.ts";
import { MATTER, roomHost, threadHost } from "./hosts.ts";
import { RoomOver } from "./states.tsx";
import { PAPERS_LIVE } from "./registry-papers-live.tsx";
import { WALL_AWAY } from "./registry-wall-away.tsx";
import { matterWords, threadWords } from "./words.ts";
import { agentText, step, THREAD, userMessage, uuid } from "../testing/stream-script.ts";
import { jumpTo } from "../room/thread/jump-entry.ts";
import { streamOf } from "../store/view.ts";

/** A retry that does nothing: the review shows the parts, the host isn't asked. */
const ACTIONS: ThreadActions = {
  stream: null,
  sentTurn: null,
  undo: async () => undefined,
  answer: async () => undefined,
  answerElicitation: async () => undefined,
  stop: () => undefined,
  retry: async () => undefined,
};

/** The lawyer's question Retry from here sits under, asked at 4:31 pm today. */
const QUESTION = "What does clause nine of the lease say about notice?";
const ASKED = new Date(2026, 10, 2, 16, 31).getTime();

/** The line a retried conversation opens with, its first conversation's title and the clock of the retry. */
const retried = () => (
  <RoomScopeProvider>
    <PartsRoot>
      <RetriedLine time="4:35 pm" title="Quashing for Mr. Sharma" onOpen={() => undefined} />
    </PartsRoot>
  </RoomScopeProvider>
);

/** Retry from here under the lawyer's question. Its button is hidden (opacity-0) until the pointer is over the
 * question or focus is on the button, as in the room; the review's stylesheet builds no utilities from this folder,
 * so nothing here forces it open. Hover the question on the page to see it. */
const retry = () => (
  <RoomScopeProvider>
    <PartsRoot>
      <ThreadActionsContext value={ACTIONS}>
        <QuestionLine who="You" time="4:31 pm" text={QUESTION} />
        <RetryFromHere message={uuid(5, 1)} at={ASKED} shown />
      </ThreadActionsContext>
    </PartsRoot>
  </RoomScopeProvider>
);

/** A matter's papers, three of them, for the new conversation's count. */
const PAPERS: PaperSummary[] = ["Plaint.pdf", "Written statement.pdf", "Lease deed.pdf"].map(
  (path, i) => ({ file: uuid(i + 1, 8), path, size: 1000, modified: 1_730_000_000_000 }),
);

/** The room on a matter with no conversation yet: a new one, before its first question. */
const fresh = () => <RoomOver host={() => roomHost([MATTER], PAPERS)} />;
/** A conversation on the ChatGPT plan whose last answer runs to the composer: where the answer ends and the plan's
 * line starts (D-073; open it with &lite=1 for lite), titled as its first question titles it (D-070), on the rail. */
const ASKED_LONG =
  "Without using web search, write a short note on the notice period in clause 9 of the lease deed";
const LONG = [
  "Clause nine needs thirty days' written notice to the bank, sent by registered post to its branch.",
  "The notice must name the loan account, the date of default and the sum outstanding on that date.",
  "Clause ten lets the bank cure within fifteen days of the notice; the lease runs on if it does.",
  "Clause eleven keeps the deposit with the lessor until the bank's charge is released in writing.",
  "Nothing in the lease lets the lessor end it without that notice, whatever the default.",
].join("\n\n");
const ended = () => (
  <RoomOver
    host={() =>
      threadHost({
        agent: "codex-acp",
        title: titleOf(ASKED_LONG) ?? ASKED_LONG,
        entries: [userMessage(5, 1, ASKED_LONG), agentText(6, 1, LONG), step(7)],
      })
    }
  />
);
/** A Find result's jump to a long answer, earlier in the conversation than the pinned question (D-071): where its
 * first line lands against the pin. */
const jumped = () => {
  jumpTo(streamOf(THREAD), 6);
  return (
    <RoomOver
      host={() =>
        threadHost({
          agent: "codex-acp",
          entries: [
            userMessage(5, 1, ASKED_LONG),
            agentText(6, 1, `${LONG}\n\n${LONG}`),
            step(7),
            userMessage(8, 2, "And what if the bank doesn't cure in time?"),
            agentText(9, 2, "Then the lessor may end the lease by a second written notice."),
            step(10),
          ],
        })
      }
    />
  );
};
/** A long answer whose table, far down it, holds the words a Find result found: clause 21's row of 24. */
const TABLED = [
  LONG,
  ["| Clause | What it says |", "| --- | --- |"]
    .concat(
      Array.from({ length: 24 }, (_, i) =>
        i === 20
          ? "| 21 | Forfeiture on a second default, after written notice |"
          : `| ${i + 1} | Terms of clause ${i + 1}, as filed with the deed |`,
      ),
    )
    .join("\n"),
  "Each clause reads with the schedule of payments annexed to the deed.",
].join("\n\n");
/** A Find result's jump to words deep in a long answer, in its table's 21st row (D-071): the row lands mid-way between
 * the pinned question and the composer, not the answer's first line. */
const jumpedDeep = () => {
  jumpTo(streamOf(THREAD), 6, [
    { text: "| 21 | ", hit: false },
    { text: "Forfeiture", hit: true },
    { text: " on a second default, after written notice |", hit: false },
  ]);
  return (
    <RoomOver
      host={() =>
        threadHost({
          agent: "codex-acp",
          entries: [
            userMessage(5, 1, ASKED_LONG),
            agentText(6, 1, TABLED),
            step(7),
            userMessage(8, 2, "And what if the bank doesn't cure in time?"),
            agentText(9, 2, "Then the lessor may end the lease by a second written notice."),
            step(10),
          ],
        })
      }
    />
  );
};
/** The same with one paper in the matter. */
const freshOne = () => <RoomOver host={() => roomHost([MATTER], PAPERS.slice(0, 1))} />;

/** U6's entries, each id written as it is: an id never moves when an entry is added or taken out. */
export const U6: Entry[] = [
  {
    id: "U6-01",
    words: matterWords(
      "Retried from your question at {time}. The first answer stays in '{title}'.",
    ),
    screen: "matters/retry",
    render: retried,
  },
  {
    id: "U6-02",
    words: matterWords("Open it"),
    screen: "matters/retry",
    render: retried,
  },
  {
    id: "U6-03",
    words: matterWords("Retry from here"),
    screen: "matters/retry",
    render: retry,
  },
  {
    id: "U6-04",
    words: matterWords(
      "Retry from here starts a new conversation from this question. Nothing is deleted.",
    ),
    screen: "matters/retry",
    render: retry,
  },
  {
    id: "U6-05",
    words: matterWords("A new conversation. It reads this matter's {n} papers."),
    screen: "matters/new-conversation",
    render: fresh,
  },
  {
    id: "U6-06",
    words: threadWords("Uses your {name} plan"),
    screen: "matters/new-conversation",
    render: fresh,
  },
  {
    id: "U6-07",
    words: matterWords("A new conversation. It reads this matter's one paper."),
    screen: "matters/new-conversation",
    render: freshOne,
  },
  {
    id: "U6-08",
    words: threadWords("Uses your {name} plan"),
    screen: "matters/thread-end",
    render: ended,
    width: 1400,
  },
  {
    id: "U6-09",
    words: threadWords("Uses your {name} plan"),
    screen: "matters/find-jump",
    render: jumped,
  },
  {
    id: "U6-10",
    words: threadWords("Uses your {name} plan"),
    screen: "matters/find-jump-deep",
    render: jumpedDeep,
  },
  // The wall following the open matter's folder (0055): PL-01 to PL-03.
  ...PAPERS_LIVE,
  // The wall's dimmed tiles, files only online and folders not opened: WA-01 to WA-19.
  ...WALL_AWAY,
];
