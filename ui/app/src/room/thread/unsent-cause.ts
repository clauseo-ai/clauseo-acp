// Why a message never started, as the host says it (TurnEnded.cause, carried on the user-message entry as `unsent`;
// the status is removed), and the copy the host queued of one held for an update or a sign-in (`copy_of` on the
// copy's own entry, naming its original, which an old log may lack). Read here, and only here: absent on older logs,
// and an unknown value (a newer host's, read as "unknown") reads as absent, when the wording falls back to this
// window's own Stop record (unsent-restore.ts).
import type { StreamView } from "../../store/view.ts";
import type { UnsentCause, UserMessageEntry } from "../../transport/protocol.ts";

/** The causes the app words; UnsentCause is open, so "unknown" (and any value this version doesn't know) reads as
 * none. */
export type Cause = Exclude<UnsentCause, "unknown">;

const CAUSES: readonly string[] = [
  "stopped",
  "removed",
  "sent_now",
  "sign_in",
  "update",
  "not_sent",
];

/** Why the message never started; null when the host doesn't say, or says something this version doesn't know. */
export function causeOf(m: UserMessageEntry): Cause | null {
  const cause: unknown = m.unsent;
  return typeof cause === "string" && CAUSES.includes(cause) ? (cause as Cause) : null;
}

/** Whether the message ended before its turn started: removed. */
export const endedUnsent = (m: UserMessageEntry): boolean => m.status === "removed";

/** Whether its words come back to the lawyer's box: not for one held for a sign-in or an update, whose copy the host
 * queues; for every other cause, and for none. */
export const givesBack = (cause: Cause | null): boolean =>
  cause !== "sign_in" && cause !== "update";

/** What a queued copy stands in for: the original held for an update or a sign-in (copy_of.cause); null for
 * any other message, or a cause this version doesn't know. Its original need not be in the view. */
export function copyCauseOf(m: UserMessageEntry): "update" | "sign_in" | null {
  const cause = m.copy_of?.cause;
  return cause === "update" || cause === "sign_in" ? cause : null;
}

/** The messages (their ids) the view's entries say they copy (copy_of.message), whatever the cause: an original
 * with a copy queued doesn't come back to the box. */
export function copiedIn(v: StreamView): ReadonlySet<string> {
  const out = new Set<string>();
  for (const id of v.ids) {
    const original = v.entries.get(id)?.user_message?.copy_of?.message;
    if (original !== undefined) out.add(original);
  }
  return out;
}
