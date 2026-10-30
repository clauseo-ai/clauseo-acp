// What the queue over the composer holds, shared by its part and the hosts that fill it.

export type QueuedMessage = { id: string; text: string };

/** Why the queue holds: the lawyer stopped the answer, or it failed, at a clock time in English. */
export type QueueHold = { kind: "stopped" | "failed"; at: string };

/** How many messages the queue holds. */
export const QUEUE_CAP = 10;
