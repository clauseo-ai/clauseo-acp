// The app's own line for a host notice, by its kind (NoticeEntry.kind): the words the lawyer reads in place of the
// host's text. An agent's notice has no kind; a kind this version doesn't know (a newer host's, arriving as some other
// string or as an object it doesn't read) gives null, and the row shows the notice's text as the host wrote it.
import type { useThreadWords } from "@clauseo/components/lib/words-thread.ts";
import type { NoticeKind } from "../../transport/protocol.ts";

/** The thread's words the line is drawn with: its page language, the thread's own, and the shared ones ({n} more). */
export type NoticeWords = Pick<ReturnType<typeof useThreadWords>, "lang" | "th" | "t">;

/** The line for a notice's kind in the page's language, its slots filled; null for no kind, or a kind this version
 * does not know, so the notice's own text shows. */
export function noticeLine(kind: NoticeKind | undefined, w: NoticeWords): string | null {
  // read as unknown: a kind is a string, or (once a newer host sends one) an object with the kind's name as key
  const k: unknown = kind;
  if (typeof k === "string")
    return k === "context_not_restored"
      ? w.th(
          "Clauseo doesn't remember the earlier questions and answers in this conversation. Include what it needs from them in your next question.",
        )
      : null;
  // no object kind is known yet: a kind of that shape reads as unknown here, and its text shows
  if (typeof k === "object" && k !== null && "files_came_back" in k) return null;
  return null;
}
