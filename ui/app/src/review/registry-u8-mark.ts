// The host's Word tool's answers for registry-u8.tsx's scripted host (word_blackline), apart from it to keep that file
// under the size limit.
import type {
  BlacklineNotice,
  BlacklinePart,
  BlacklineParagraph,
  ChangedTable,
  QueryResult,
} from "../transport/protocol.ts";

/** word_blackline's answer for a Word document compared with now: marked up (by default, whole), with notices,
 * links whose web address changed or paragraphs of its own, left unanswered, or refused with a code. */
export type Mark =
  | "hold"
  | { truncated: boolean }
  | { notices: BlacklineNotice[]; changed_tables?: ChangedTable[] }
  | { parts: BlacklinePart[] }
  | { changed_links: string[] }
  | { paragraphs: BlacklineParagraph[] }
  | { refused: string; retryable: boolean };

/** The host's Word tool's marking up of the notice: a word out and a word in, and a paragraph added. */
const MARKED_NOTICE: BlacklineParagraph[] = [
  {
    change: "same",
    runs: [{ kind: "same", text: "To the Branch Manager, State Bank of India, Connaught Place." }],
  },
  {
    change: "changed",
    runs: [
      { kind: "same", text: "My client asks that the account be restored within " },
      { kind: "del", text: "thirty" },
      { kind: "ins", text: "fifteen" },
      { kind: "same", text: " days of this notice." },
    ],
  },
  {
    change: "added",
    runs: [{ kind: "ins", text: "A copy of the statement of 3 March is annexed." }],
  },
];

/** word_blackline's answer, as `mark` asks. */
export function marking(mark: Mark | undefined): QueryResult | Promise<QueryResult> {
  if (mark === "hold") return new Promise<QueryResult>(() => {});
  if (mark !== undefined && "refused" in mark)
    return Promise.reject({ code: mark.refused, message: "", retryable: mark.retryable });
  const truncated = mark !== undefined && "truncated" in mark && mark.truncated;
  const notices = mark !== undefined && "notices" in mark ? mark.notices : [];
  const paragraphs = mark !== undefined && "paragraphs" in mark ? mark.paragraphs : MARKED_NOTICE;
  const changed_links = mark !== undefined && "changed_links" in mark ? mark.changed_links : [];
  const parts = mark !== undefined && "parts" in mark ? mark.parts : [];
  const changed_tables =
    mark !== undefined && "changed_tables" in mark ? (mark.changed_tables ?? []) : [];
  return {
    word_blackline: { paragraphs, truncated, notices, changed_links, parts, changed_tables },
  };
}
