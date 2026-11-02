// U6's copy review entries: apart from registry.tsx, as registry-u4.tsx is. Retrying from a question, each string
// over the parts that show it, on their own, inside the room's providers: the room doesn't yet make a retry (H13
// lands later), so no room state shows these. A new conversation's line and the status line's plan, in the room over
// a host with H8's papers and no threads yet, so the matter opens on a new conversation.
import { QuestionLine } from "@clauseo/components/clauseo/answer.tsx";
import { RoomScopeProvider } from "@clauseo/components/clauseo/room-scope-provider.tsx";
import { PartsRoot } from "@clauseo/components/ui/root.tsx";
import { RetriedLine } from "../room/retried-line.tsx";
import { RetryFromHere } from "../room/retry-from-here.tsx";
import { ThreadActionsContext, type ThreadActions } from "../room/thread/session.ts";
import type { Entry } from "./entry.ts";
import type { PaperSummary } from "../room/host-next.ts";
import { MATTER, roomHost } from "./hosts.ts";
import { RoomOver } from "./states.tsx";
import { matterWords, threadWords } from "./words.ts";
import { uuid } from "../testing/stream-script.ts";

/** A retry that does nothing: the review shows the parts, the host isn't asked. */
const ACTIONS: ThreadActions = {
  stream: null,
  sentTurn: null,
  undo: async () => undefined,
  answer: async () => undefined,
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
];
