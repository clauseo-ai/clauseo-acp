// Why a message never started, as the host says it (#1314: TurnEnded.cause, carried on the user-message entry as
// `unsent`; the status stays removed), and the copy the host queued of one held for an update or a sign-in
// (`copy_of` on the copy's own entry, naming its original, which an old log may lack). Read here, and only here:
// absent on older logs and until #1314 lands, and an unknown value reads as absent, when the wording falls back to
// this window's own Stop record (unsent-restore.ts).
import type { StreamView } from "../../store/view.ts";
import type { UserMessageEntry } from "../../transport/protocol.ts";

/** The causes #1314 names; its enum is open, so any other value reads as none. */
export type Cause = "stopped" | "removed" | "sent_now" | "sign_in" | "update" | "not_sent";

/** The entry as #1314 gives it. TODO(#1314): a stand-in until that binding is on main, when UserMessageEntry carries
 * `unsent` and `copy_of` itself and this becomes UserMessageEntry. */
type CausedEntry = UserMessageEntry & {
  unsent?: string;
  copy_of?: { message: string; cause: string };
};

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
  const cause: unknown = (m as CausedEntry).unsent;
  return typeof cause === "string" && CAUSES.includes(cause) ? (cause as Cause) : null;
}

/** Whether the message ended before its turn started: removed (and, should a host say so, not_sent). */
export function endedUnsent(m: UserMessageEntry): boolean {
  const status: string = m.status;
  return status === "removed" || status === "not_sent";
}

/** Whether its words come back to the lawyer's box: not for one held for a sign-in or an update, whose copy the host
 * queues; for every other cause, and for none. */
export const givesBack = (cause: Cause | null): boolean =>
  cause !== "sign_in" && cause !== "update";

/** What a queued copy stands in for: the original held for an update or a sign-in (#1314's copy_of.cause); null for
 * any other message, or a cause this version doesn't know. Its original need not be in the view. */
export function copyCauseOf(m: UserMessageEntry): "update" | "sign_in" | null {
  const cause: unknown = (m as CausedEntry).copy_of?.cause;
  return cause === "update" || cause === "sign_in" ? cause : null;
}

/** The messages (their ids) the view's entries say they copy (copy_of.message), whatever the cause: an original
 * with a copy queued doesn't come back to the box. */
export function copiedIn(v: StreamView): ReadonlySet<string> {
  const out = new Set<string>();
  for (const id of v.ids) {
    const m = v.entries.get(id)?.user_message as CausedEntry | undefined;
    const original: unknown = m?.copy_of?.message;
    if (typeof original === "string") out.add(original);
  }
  return out;
}
