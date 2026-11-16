// U3's entries for a turn's failure that is never a send's refusal: going offline mid-answer (D-050), the agent's
// service giving up on its side (agent.service_error, #1030), and the agent going quiet for 5 minutes (agent.quiet,
// D-050's backstop), the agent unable to reach its service while the computer seemed online (agent.unreachable,
// offline truth), a folder Claude can't start in (matter.folder_backslash, agent.folder_inaccessible) and Claude's
// protection unavailable (registry-sandbox-off.tsx) or refused over its settings (registry-claude-settings.tsx); and which
// error codes need no send-refusal entry, split from registry.tsx for its length.
import { MARK_KEYS, type ErrorKey } from "@clauseo/components/lib/words-errors.ts";
import { agentText, step, userMessage } from "../testing/stream-script.ts";
import type { ErrorCode } from "../transport/protocol.ts";
import type { Entry } from "./entry.ts";
import { threadHost } from "./hosts.ts";
import { RoomOver } from "./states.tsx";
import { CLAUDE_SETTINGS } from "./registry-claude-settings.tsx";
import { SANDBOX_OFF } from "./registry-sandbox-off.tsx";
import { errorWords } from "./words.ts";

/** Whether an error code's sentence needs no "U3-err-<code>" refusal on a send: Versions' own (the Word blackline's,
 * words-errors.ts's MARK_KEYS, UC-114 and UC-115), and agent.offline, agent.service_error, agent.quiet,
 * agent.unreachable, matter.folder_backslash and agent.folder_inaccessible, a turn's failures with their own entries
 * below, and the sandbox refusals' (registry-sandbox-off.tsx) and Claude's settings ones (registry-claude-settings.tsx). Only the false answer narrows: a key it refuses is never one of MARK_KEYS. */
export const ownEntry = (code: ErrorCode, key: ErrorKey): key is (typeof MARK_KEYS)[number] =>
  code === "agent.offline" ||
  code === "agent.service_error" ||
  code === "agent.quiet" ||
  code === "agent.unreachable" ||
  code === "matter.folder_backslash" ||
  code === "agent.folder_inaccessible" ||
  Object.hasOwn(SANDBOX_OFF, code) ||
  Object.hasOwn(CLAUDE_SETTINGS, code) ||
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

/** agent.quiet is never a send's refusal either: Claude's stop line under the answer so far, with Try again and never
 * Restart. ChatGPT's, an agent's by its name and the nameless line are U6F-22 to U6F-24. */
const QUIET: Entry[] = [
  {
    id: "U3-err-agent.quiet",
    words: errorWords("Claude stopped answering. Try again."),
    screen: "room/thread",
    render: () => (
      <RoomOver
        host={() =>
          threadHost({
            entries: [
              userMessage(5, 1, "What does clause nine of the lease say about notice?"),
              agentText(6, 1, "Clause nine requires **notice in writing**, served at"),
              step(9, { stop: "failed", failure: "agent.quiet" }),
            ],
          })
        }
      />
    ),
  },
];

/** agent.unreachable is never a send's refusal either: Claude's stop line under the answer so far, with Try again and
 * never Restart (UC-530, offline truth). ChatGPT's, an agent's by its name and the nameless line are UC-531 to
 * UC-533. */
const UNREACHABLE: Entry[] = [
  {
    id: "UC-530",
    words: errorWords(
      "Clauseo couldn't reach Claude. The answer so far is kept. Try again. If this keeps happening, ask your firm's IT.",
    ),
    screen: "room/thread",
    render: () => (
      <RoomOver
        host={() =>
          threadHost({
            entries: [
              userMessage(5, 1, "What does clause nine of the lease say about notice?"),
              agentText(6, 1, "Clause nine requires **notice in writing**, served at"),
              step(9, { stop: "failed", failure: "agent.unreachable" }),
            ],
          })
        }
      />
    ),
  },
];

/** A turn Claude couldn't start in the matter's folder, never a send's refusal: the question and the stop line under it,
 * with Try again and never Restart. Draft English (Rohan to approve). ChatGPT's, an agent's by its name and the
 * nameless line are U6F-25 to U6F-30. */
const folderStop = (code: ErrorCode, key: Parameters<typeof errorWords>[0]): Entry[] => [
  {
    id: `U3-err-${code}`,
    words: errorWords(key),
    screen: "room/thread",
    render: () => (
      <RoomOver
        host={() =>
          threadHost({
            entries: [
              userMessage(5, 1, "What does clause nine of the lease say about notice?"),
              step(9, { stop: "failed", failure: code }),
            ],
            // matter.folder_backslash's line as on the Mac, for the matter folder's own name
            platform: "mac",
            matterFolder: "Documents/Acme\\ Traders",
          })
        }
      />
    ),
  },
];

/** The entries in place of a refusal for a code `ownEntry` names, in their code's place. */
const OWN: Partial<Record<ErrorCode, Entry[]>> = {
  "agent.offline": OFFLINE,
  "agent.service_error": SERVICE,
  "agent.quiet": QUIET,
  "agent.unreachable": UNREACHABLE,
  "matter.folder_backslash": folderStop(
    "matter.folder_backslash",
    "Claude can't work in this matter's folder because its name has a backslash (\\). Rename the folder in Finder, then Try again.",
  ),
  "agent.folder_inaccessible": folderStop(
    "agent.folder_inaccessible",
    "Claude couldn't open the matter's folder. Check that it's still there and hasn't been renamed, then Try again.",
  ),
  // Claude's protection couldn't be set up (UC-470 to UC-472)
  ...SANDBOX_OFF,
  // Claude refused or stopped over its settings (UC-474 to UC-476)
  ...CLAUDE_SETTINGS,
};

/** The entries in place of a refusal for a code `ownEntry` names: agent.offline's, agent.service_error's, agent.quiet's,
 * agent.unreachable's and the two folder codes' own, in their code's place. */
export const ownEntries = (code: ErrorCode): Entry[] => OWN[code] ?? [];
