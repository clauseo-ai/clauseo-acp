// U3's entries for a turn's failure that is never a send's refusal: going offline mid-answer (D-050), and the agent's
// service giving up on its side (agent.service_error, #1030); and which error codes need no send-refusal entry, split
// from registry.tsx for its length.
import { MARK_KEYS, type ErrorKey } from "@clauseo/components/lib/words-errors.ts";
import { agentText, step, userMessage } from "../testing/stream-script.ts";
import type { ErrorCode } from "../transport/protocol.ts";
import type { Entry } from "./entry.ts";
import { threadHost } from "./hosts.ts";
import { RoomOver } from "./states.tsx";
import { errorWords } from "./words.ts";

/** Whether an error code's sentence needs no "U3-err-<code>" refusal on a send: Versions' own (the Word blackline's,
 * words-errors.ts's MARK_KEYS, UC-114 and UC-115), and agent.offline and agent.service_error, a turn's failures with
 * their own entries below. Only the false answer narrows: a key it refuses is never one of MARK_KEYS. */
export const ownEntry = (code: ErrorCode, key: ErrorKey): key is (typeof MARK_KEYS)[number] =>
  code === "agent.offline" ||
  code === "agent.service_error" ||
  (MARK_KEYS as readonly string[]).includes(key);

/** agent.offline is never a send's refusal: the stop line under the answer so far, with Try again. */
const OFFLINE: Entry[] = [
  {
    id: "U3-err-agent.offline",
    words: errorWords(
      "You're offline, so the answer stopped. The answer so far is kept. Try again when you're back online.",
    ),
    screen: "room/thread",
    render: () => (
      <RoomOver
        host={() =>
          threadHost({
            entries: [
              userMessage(5, 1, "What does clause nine of the lease say about notice?"),
              agentText(6, 1, "Clause nine requires **notice in writing**, served at"),
              step(9, { stop: "failed", failure: "agent.offline" }),
            ],
          })
        }
      />
    ),
  },
];

/** agent.service_error is never a send's refusal either: Claude's stop line under the answer so far, with Try again and
 * never Restart. ChatGPT's, an agent's by its name and the nameless line are U6F-19 to U6F-21. */
const SERVICE: Entry[] = [
  {
    id: "U3-err-agent.service_error",
    words: errorWords("Claude had a problem on its side. Try again."),
    screen: "room/thread",
    render: () => (
      <RoomOver
        host={() =>
          threadHost({
            entries: [
              userMessage(5, 1, "What does clause nine of the lease say about notice?"),
              agentText(6, 1, "Clause nine requires **notice in writing**, served at"),
              step(9, { stop: "failed", failure: "agent.service_error" }),
            ],
          })
        }
      />
    ),
  },
];

/** The entries in place of a refusal for a code `ownEntry` names: agent.offline's and agent.service_error's own, in
 * their code's place. */
export const ownEntries = (code: ErrorCode): Entry[] =>
  code === "agent.offline" ? OFFLINE : code === "agent.service_error" ? SERVICE : [];
