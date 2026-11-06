// What the queue over the composer holds, shared by its part and the hosts that fill it.

/** One queued message: its id and words, and the turn it will be (a host's, for a test hook; none in the design). */
export type QueuedMessage = { id: string; text: string; turn?: string | undefined };

/** Why the queue holds: the lawyer stopped the answer, or didn't allow a request, or it failed, at a clock time in
 * English; or held with no stop to name (after a restart, or at a limit), when the head keeps its count line and
 * offers Send next. */
export type QueueHold = (
  | { kind: "stopped" | "not_allowed" | "failed"; at: string }
  | { kind: "held" }
) & {
  /** Why the host holds it (`queue_held_reason`: sign_in, switching, update, limit, paused, and more to come); absent
   * from an older host. Send next shows only without one or for the lawyer's own pause: under any other hold the host
   * starts nothing, so the head keeps its count line, and an update's hold says so beneath it. */
  reason?: string | undefined;
};

/** How many messages the queue holds. */
export const QUEUE_CAP = 10;

/** data-testid hooks on the strip's parts, for a host's tests; none by default. */
export type QueueTestIds = {
  /** Each row, which carries `data-turn` as well. */
  row?: string;
  edit?: string;
  remove?: string;
  sendNow?: string;
  sendNext?: string;
  /** The head's status line. */
  say?: string;
  /** The line under the head for a refused press. */
  note?: string;
  /** The line under the head that says why the host holds the queue (an update). */
  why?: string;
};
