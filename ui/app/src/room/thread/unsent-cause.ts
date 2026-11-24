// Why a message never started, as the host says it (#1314: TurnEnded.cause, carried on the user-message entry as
// `unsent`; status not_sent for sent_now and not_sent, removed for the rest). Read here, and only here: absent on
// older logs and until #1314 lands, and an unknown value reads as absent, when the wording falls back to this window's
// own Stop record (unsent-restore.ts).
import type { UserMessageEntry } from "../../transport/protocol.ts";

/** The causes #1314 names. */
export type Cause = "stopped" | "removed" | "sent_now" | "sign_in" | "update" | "not_sent";

/** The entry as #1314 gives it. A stand-in until that binding is on main, when UserMessageEntry carries `unsent`
 * (and UserMessageStatus "not_sent") itself. */
type CausedEntry = UserMessageEntry & { unsent?: string };

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

/** Whether the message ended before its turn started: removed, or (#1314) not sent. */
export function endedUnsent(m: UserMessageEntry): boolean {
  const status: string = m.status;
  return status === "removed" || status === "not_sent";
}

/** Whether its words come back to the lawyer: not for one the lawyer removed from the queue, nor for one whose copy
 * waits in the queue (sign-in, update). */
export const givesBack = (cause: Cause | null): boolean =>
  cause !== "removed" && cause !== "sign_in" && cause !== "update";
