// Host notices' copy review entries (TN-01 on), apart from registry.tsx to keep that file under the size limit: a
// notice the host sends with its kind, in the room's thread, shown in the app's words (notice-words.ts). Each over the
// thread the notice is in, as the host sends it.
import type { ReactNode } from "react";
import { uuid, userMessage } from "../testing/stream-script.ts";
import type { NoticeEntry, ViewEntry } from "../transport/protocol.ts";
import type { Entry } from "./entry.ts";
import { threadHost } from "./hosts.ts";
import { RoomOver } from "./states.tsx";
import { threadWords } from "./words.ts";

const CONTEXT =
  "Clauseo doesn't remember the earlier questions and answers in this conversation. Include what it needs from them in your next question.";
const ONE_BACK = "1 paper came back after the undo: {papers}. Clauseo didn't remove it again.";
const MANY_BACK =
  "{n} papers came back after the undo: {papers}. Clauseo didn't remove them again.";

/** The lawyer's question, then the host's notice. */
const thread =
  (notice: ViewEntry): (() => ReactNode) =>
  () => (
    <RoomOver
      host={() =>
        threadHost({
          entries: [
            userMessage(5, 1, "What does clause nine of the lease say about notice?"),
            notice,
          ],
        })
      }
    />
  );

/** A host notice at `order`, with its kind and text as given. */
const notice = (order: number, over: Partial<NoticeEntry>): ViewEntry => ({
  notice: {
    order,
    event: uuid(1, 10),
    level: "info",
    text: "Papers came back after the undo.",
    ts: 6000,
    ...over,
  },
});

const CONTEXT_NOTICE = notice(6, { text: CONTEXT, kind: "context_not_restored" });
/** One paper a sync client put back after the undo. */
const ONE_PAPER = notice(6, {
  kind: { files_came_back: { files: ["Pleadings/Reply.docx"], more: 0 } },
});
/** Five papers and three more; two of them share the name Reply.docx, so the list shows their whole paths. */
const MANY_PAPERS = notice(6, {
  kind: {
    files_came_back: {
      files: [
        "Drafts/Reply.docx",
        "Lease/Rent review.docx",
        "Lease/Schedule 2.docx",
        "Notice to the bank.docx",
        "Pleadings/Reply.docx",
      ],
      more: 3,
    },
  },
});

/** The notices' entries: "TN-01" and so on, each id written as it is. */
export const TN: Entry[] = [
  {
    id: "TN-01",
    words: threadWords(CONTEXT),
    screen: "room/thread",
    render: thread(CONTEXT_NOTICE),
  },
  {
    id: "TN-02",
    words: threadWords(ONE_BACK),
    screen: "room/thread",
    render: thread(ONE_PAPER),
  },
  {
    id: "TN-03",
    words: threadWords(MANY_BACK),
    screen: "room/thread",
    render: thread(MANY_PAPERS),
  },
];
