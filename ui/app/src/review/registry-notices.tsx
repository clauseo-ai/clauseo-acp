// Host notices' copy review entries (TN-01 on; UC-346 to UC-350 the read invite's: another matter's files refused, with
// the offer, and matters Claude's commands may reach), apart from registry.tsx to keep that file under the size limit: a
// notice the host sends with its kind, in the room's thread, shown in the app's words (notice-words.ts). Each over the
// thread the notice is in, as the host sends it.
import type { ReactNode } from "react";
import type { Lang } from "@clauseo/components/lib/display.ts";
import { listOf } from "@clauseo/components/lib/words-core.ts";
import { th, type ThreadKey } from "@clauseo/components/lib/words-thread.ts";
import { t } from "@clauseo/components/lib/words.ts";
import { uuid, userMessage } from "../testing/stream-script.ts";
import type {
  MatterSummary,
  NoticeEntry,
  PackSkipReason,
  ViewEntry,
} from "../transport/protocol.ts";
import type { Entry } from "./entry.ts";
import { threadHost } from "./hosts.ts";
import { RoomOver } from "./states.tsx";
import { threadWords } from "./words.ts";

const CONTEXT =
  "Clauseo doesn't remember the earlier questions and answers in this conversation. Include what it needs from them in your next question.";
const AUTO_UNAVAILABLE =
  "Claude's automatic mode isn't available for this account, so Claude asks before running commands.";
const ONE_BACK = "1 paper came back after the undo: {papers}. Clauseo didn't remove it again.";
const MANY_BACK =
  "{n} papers came back after the undo: {papers}. Clauseo didn't remove them again.";
/** A skill pack the launch left out, one line per reason (TN-04 to TN-09); the pack is "nda-kit" in each. */
const PACK_LINE: Record<Exclude<PackSkipReason, "unknown">, ThreadKey> = {
  not_turned_on:
    "Clauseo didn't load the skill pack '{pack}'. This matter is set to use it, but it isn't turned on.",
  not_admitted:
    "Clauseo didn't load the skill pack '{pack}'. The skill pack settings on this computer don't allow it.",
  changed:
    "Clauseo didn't load the skill pack '{pack}'. Its files changed after it was turned on. Turn it on again to use it.",
  missing:
    "Clauseo didn't load the skill pack '{pack}'. Clauseo couldn't find it on this computer.",
  invalid: "Clauseo didn't load the skill pack '{pack}'. Its files aren't a valid skill pack.",
  too_many:
    "Clauseo didn't load the skill pack '{pack}'. Clauseo loads at most 16 skill packs at a time.",
};

/** A skill pack the launch left out, for a reason, as the host sends it. */
const packSkipped = (reason: PackSkipReason): ViewEntry =>
  notice(6, {
    text: "A skill pack was left out of this launch.",
    kind: { skill_pack_not_loaded: { pack: "nda-kit", reason } },
  });

/** Other matters in the list, for the notices that name one. */
const JOSHI: MatterSummary = { matter: uuid(2, 6), name: "Joshi v. Joshi", kind: "folder" };
const KAPOOR: MatterSummary = {
  matter: uuid(3, 6),
  name: "Kapoor Exports v. State Bank of India",
  kind: "folder",
};

/** The lawyer's question, then the host's notice, in a thread of claude-acp; the list holds Joshi's and Kapoor's
 * matters beside the thread's own. */
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
          others: [JOSHI, KAPOOR],
        })
      }
    />
  );

/** A line rendered alone, as the room would draw it where it knows no agent name. */
const alone =
  (line: (lang: Lang) => string) =>
  (lang: Lang): ReactNode => (
    <main lang={lang} className="review-alone">
      <p>{line(lang)}</p>
    </main>
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
/** Claude's auto mode isn't available, so the session runs in acceptEdits. */
const AUTO_NOTICE = notice(6, { text: AUTO_UNAVAILABLE, kind: "auto_mode_unavailable" });
const BLOCKED = "Clauseo stopped a step: {name} tried to reach outside this matter.";
const BLOCKED_ALONE = "Clauseo stopped a step: the AI tried to reach outside this matter.";
/** Clauseo's data fence refused, unasked, a request of the agent's (once a turn). */
const BLOCKED_NOTICE = notice(6, {
  level: "warning",
  text: BLOCKED_ALONE,
  kind: "outside_matter_blocked",
});
const OTHER =
  "Clauseo stopped a step: {name} tried to open {matter}'s files. That's another matter.";
const OTHER_ALONE =
  "Clauseo stopped a step: the AI tried to open {matter}'s files. That's another matter.";
const OFFER = "Let this conversation read {matter}'s files";
/** Clauseo kept Claude out of Joshi's files: the line, and the offer to let this conversation read them. */
const OTHER_NOTICE = notice(6, {
  level: "warning",
  text: "Clauseo kept the agent out of another matter's files.",
  kind: { other_matter_refused: { matter: JOSHI.matter, name: JOSHI.name } },
});
const OFFLINE_NO_SEARCH = "Clauseo stopped a step: {name} tried to go online by running a command.";
const OFFLINE_NO_SEARCH_ALONE =
  "Clauseo stopped a step: the AI tried to go online by running a command.";
/** The same, in a session without web search: it offers none. */
const OFFLINE_NO_SEARCH_NOTICE = notice(6, {
  level: "warning",
  text: "Clauseo stopped a step: ChatGPT tried to go online by running a command.",
  kind: "shell_network_blocked_no_web_search",
});
const UNFENCED = "Clauseo couldn't keep {name}'s commands out of these matters: {matters}.";
const UNFENCED_ALONE = "Clauseo couldn't keep the AI's commands out of these matters: {matters}.";
/** Claude on Windows: two matters named, and three more. */
const UNFENCED_NOTICE = notice(6, {
  level: "warning",
  text: "Clauseo couldn't keep the agent's commands out of some matters.",
  kind: { matters_unfenced: { matters: [JOSHI.matter, KAPOOR.matter], more: 3 } },
});
const iso = (name: string) => `\u2068${name}\u2069`;
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
  ...(
    [
      ["TN-04", "not_turned_on"],
      ["TN-05", "not_admitted"],
      ["TN-06", "changed"],
      ["TN-07", "missing"],
      ["TN-08", "invalid"],
      ["TN-09", "too_many"],
    ] as const
  ).map(([id, reason]): Entry => ({
    id,
    words: threadWords(PACK_LINE[reason]),
    screen: "room/thread",
    render: thread(packSkipped(reason)),
  })),
  {
    id: "TN-10",
    words: threadWords(AUTO_UNAVAILABLE),
    screen: "room/thread",
    render: thread(AUTO_NOTICE),
  },
  {
    id: "TN-11",
    words: threadWords(BLOCKED),
    screen: "room/thread",
    render: thread(BLOCKED_NOTICE),
  },
  {
    // Where the room knows no agent name, which it never has today: rendered alone, as U3-53.
    id: "TN-12",
    words: threadWords(BLOCKED_ALONE),
    screen: "room/thread",
    render: alone((lang) => th(lang, BLOCKED_ALONE)),
  },
  {
    id: "UC-346",
    words: threadWords(OTHER),
    screen: "room/thread",
    render: thread(OTHER_NOTICE),
  },
  {
    // Where the room knows no agent name: rendered alone, as TN-12.
    id: "UC-347",
    words: threadWords(OTHER_ALONE),
    screen: "room/thread",
    render: alone((lang) => th(lang, OTHER_ALONE, { matter: iso(JOSHI.name) })),
  },
  {
    // The offer under UC-346's line, while the matter is in the list and not yet allowed.
    id: "UC-348",
    words: threadWords(OFFER),
    screen: "room/thread",
    render: thread(OTHER_NOTICE),
  },
  {
    id: "UC-349",
    words: threadWords(UNFENCED),
    screen: "room/thread",
    render: thread(UNFENCED_NOTICE),
  },
  {
    id: "UC-350",
    words: threadWords(UNFENCED_ALONE),
    screen: "room/thread",
    render: alone((lang) =>
      th(lang, UNFENCED_ALONE, {
        matters: listOf(lang, [iso(JOSHI.name), iso(KAPOOR.name), t(lang, "{n} more", { n: 3 })]),
      }),
    ),
  },
  {
    id: "TN-13",
    words: threadWords(OFFLINE),
    screen: "room/thread",
    render: thread(OFFLINE_NOTICE),
  },
  {
    // Where the room knows no agent name, which it never has today: rendered alone, as TN-12.
    id: "TN-14",
    words: threadWords(OFFLINE_ALONE),
    screen: "room/thread",
    render: (lang: Lang) => (
      <main lang={lang} className="review-alone">
        <p>{th(lang, OFFLINE_ALONE)}</p>
      </main>
    ),
  },
  {
    id: "UC-570",
    words: threadWords(OFFLINE_NO_SEARCH),
    screen: "room/thread",
    render: thread(OFFLINE_NO_SEARCH_NOTICE),
  },
  {
    // Where the room knows no agent name, as TN-14: rendered alone.
    id: "UC-571",
    words: threadWords(OFFLINE_NO_SEARCH_ALONE),
    screen: "room/thread",
    render: (lang: Lang) => (
      <main lang={lang} className="review-alone">
        <p>{th(lang, OFFLINE_NO_SEARCH_ALONE)}</p>
      </main>
    ),
  },
];
