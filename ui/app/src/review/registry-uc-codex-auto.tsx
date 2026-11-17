// UC's entries for ChatGPT running in Codex's automatic mode (UC-920 to UC-923): its permission card's allow answer
// that Codex keeps for the rest of the conversation, "For this conversation", and the card folded after it; and the
// notice that the account refuses the automatic mode. The real room over FakeTransport, in a thread of codex-acp.
import { chosen, editCall, permission, userMessage, uuid } from "../testing/stream-script.ts";
import type { ViewEntry } from "../transport/protocol.ts";
import type { Entry } from "./entry.ts";
import { threadHost, type ThreadState } from "./hosts.ts";
import { RoomOver } from "./states.tsx";
import { threadWords } from "./words.ts";

const QUESTION = userMessage(5, 1, "Tighten the notice to the bank.");

/** A ChatGPT thread with the lawyer's question and then `entries`. */
const codex = (entries: ViewEntry[], status: NonNullable<ThreadState["status"]>) => () => (
  <RoomOver
    host={() => threadHost({ agent: "codex-acp", entries: [QUESTION, ...entries], status })}
  />
);

/** The host's notice that the account refuses Codex's reviewer, as the host sends it. */
const AUTO_NOTICE: ViewEntry = {
  notice: {
    order: 6,
    event: uuid(1, 10),
    level: "info",
    text: "ChatGPT's automatic mode isn't available for this account, so ChatGPT asks before approving steps.",
    ts: 6000,
    kind: "auto_mode_unavailable",
  },
};

const pending = codex([editCall(6, 1, "pending"), permission(7, 1)], "needs_you");

export const UC_CODEX_AUTO: Entry[] = [
  {
    id: "UC-920",
    words: threadWords("For this conversation"),
    screen: "room/thread",
    render: pending,
  },
  {
    id: "UC-921",
    words: threadWords("Allowed for this conversation."),
    screen: "room/thread",
    render: codex([permission(7, 3, chosen("allow_always"), null)], "idle"),
  },
  {
    id: "UC-922",
    words: threadWords("Allowed for this conversation. Clauseo saved a new version of {paper}."),
    screen: "room/thread",
    render: codex([editCall(6, 1, "completed"), permission(7, 1, chosen("allow_always"))], "idle"),
  },
  {
    id: "UC-923",
    words: threadWords(
      "ChatGPT's automatic mode isn't available for this account, so ChatGPT asks before approving steps.",
    ),
    screen: "room/thread",
    render: codex([AUTO_NOTICE], "idle"),
  },
];
