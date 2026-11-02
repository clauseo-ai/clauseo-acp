// The Word pages of the probe build (probe-main.tsx's ?word=lock, general, open and tile): the room with one
// conversation whose last turn changed papers, its change note under it, and a wall listing two Word documents. On lock
// and general the host refuses every undo_step with matter.file_locked, so pressing Undo shows the lock's lines under
// the note: Word's lines for the one Word document (lock), or the error map's line for two files (general). On open the
// note names one Word document the wall lists, so it offers "Open in Word"; on tile the probe focuses that document's
// tile on the wall, for its tooltip. Only the probe build imports this file. Not shipped.
import {
  batch,
  event,
  header,
  MATTER,
  reset,
  step,
  STREAM,
  THREAD,
  userMessage,
} from "../../testing/stream-script.ts";
import type {
  FileChange,
  HostError,
  HostEvent,
  HostQuery,
  QueryResult,
  ViewEntry,
} from "../../transport/protocol.ts";
import type { ThreadInfo } from "../host-next.ts";

/** Which Word page the probe loads: the lock's lines for one Word document, the general lock line for two files, the
 * note's "Open in Word", or the wall's Word tile with its tooltip. */
export type WordPage = "lock" | "general" | "open" | "tile";

/** ?word=… as the probe reads it; any other value is no Word page. */
export function wordPageOf(value: string | null): WordPage | null {
  return value === "lock" || value === "general" || value === "open" || value === "tile"
    ? value
    : null;
}

/** The Word document the pages are about, on the wall in place of the complaint, and the other one already there. */
export const PETITION = "Petition to quash.docx";
const REPLY = "papers/Reply to the notice.docx";

/** The wall's files on a Word page: the probe's five, the complaint swapped for the petition. */
export const wordFiles = (files: readonly string[]): string[] =>
  files.map((name) => (name === "Complaint under section 138.docx" ? PETITION : name));

/** The refusal of a program holding a file, as the host sends it. */
export const LOCKED: HostError = { code: "matter.file_locked", message: "held", retryable: true };

const file = (path: string): FileChange => ({
  path,
  change: "modified",
  size_bytes: 1200,
  may_include_lawyer_edits: false,
});

/** The lawyer's prompt, asked at `ts`. */
const asked = (ts: number): ViewEntry => {
  const entry = userMessage(2, 1, "Tighten the prayer in the petition.");
  return entry.user_message === undefined ? entry : { user_message: { ...entry.user_message, ts } };
};

/** What the probe's host answers and sends for one Word page. */
export type WordHost = {
  /** The matter's conversations, as the NextHost lists them: the one the room reopens. */
  threads: readonly ThreadInfo[];
  query: (query: HostQuery) => QueryResult | undefined;
  subscribe: (stream: string) => HostEvent[];
  /** Whether the host refuses an undo_step with the lock. */
  refusesUndo: boolean;
};

/** The host for one Word page: one idle conversation, its prompt and the step that changed the papers. */
export function wordHost(page: WordPage): WordHost {
  const paths = page === "general" ? [`papers/${PETITION}`, REPLY] : [`papers/${PETITION}`];
  const thread: ThreadInfo = {
    thread: THREAD,
    matter: MATTER,
    agent: "claude-acp",
    status: "idle",
    title: "Petition",
    seen: true,
  };
  return {
    threads: [thread],
    query: (query) =>
      typeof query === "object" && query.list_threads !== undefined
        ? { threads: [thread] }
        : undefined,
    subscribe: (stream) =>
      stream === STREAM
        ? [
            event(
              batch(
                1,
                [
                  reset(
                    [
                      asked(Date.now() - 6 * 60_000),
                      step(9, {
                        files: paths.map(file),
                        checkpoint_after: "cp9",
                        ts: Date.now() - 5 * 60_000,
                      }),
                    ],
                    header("idle"),
                  ),
                ],
                undefined,
                stream,
              ),
            ),
          ]
        : [],
    refusesUndo: page === "lock" || page === "general",
  };
}
