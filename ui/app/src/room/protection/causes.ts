// UC: what a Protection says, as the room's line, the marks and the firm's hold read it, in the
// room's first load. The wire's NotProtectedCause is open (a newer Clauseo's cause decodes as a
// plain string). A cause is read here only for its name and group; what it carries (the pool's
// counts and why, the unsupported why, the block's reason) is detail.ts's and blocked.ts's,
// outside the first load.
import type { NotProtectedCause, Protection } from "../../transport/protocol.ts";

/** The cause groups (UC, Screens): set-up (Try again offered), can't (no retry), the pool, switched off, and repair
 * (R17 PR-E's M4: a set-up in place but broken refuses every launch, and its way out is the installer or IT, never
 * Try again). */
export type CauseGroup = "setup" | "cant" | "pool" | "off" | "repair";

/** A cause's data-cause name: the wire's, or "unknown" for a shape this version doesn't know. */
export type CauseName =
  | "needs_set_up"
  | "set_up_refused"
  | "set_up_failed"
  | "probe_failed"
  | "unsupported"
  | "unknown"
  | "pool_full"
  | "switched_off"
  | "blocked"
  | "other_account"
  | "pool_owned";

/** One cause, decoded: its name and group, and the wire's value for detail.ts and blocked.ts. */
export type Cause = { name: CauseName; group: CauseGroup; raw: NotProtectedCause };

const GROUP: Partial<Record<string, CauseGroup>> = {
  needs_set_up: "setup",
  set_up_refused: "setup",
  // M4: a failed set-up or check refuses every launch whatever the settings; the repair, not a retry, mends it
  set_up_failed: "repair",
  // #722: the check that the Safe workspace works couldn't confirm it; it refuses every launch, and Try again
  // (retry_protection) checks again, as a set-up's retry does
  probe_failed: "setup",
  unsupported: "cant",
  pool_full: "pool",
  switched_off: "off",
  // R17 PR-E: something on this computer stops the Safe workspace (blocked { reason }), another
  // Windows account owns it, or an item a place didn't hand back (pool_owned); none offers Try
  // again, and blocked.ts and the note say each in words
  blocked: "cant",
  other_account: "cant",
  pool_owned: "cant",
};

/** A NotProtectedCause decoded; any shape this version doesn't know reads as can't (UC, Decided here). */
export function causeOf(cause: NotProtectedCause): Cause {
  const key = typeof cause === "object" && cause !== null ? Object.keys(cause)[0] : cause;
  const group = Object.hasOwn(GROUP, key ?? "") ? GROUP[key as string] : undefined;
  return group === undefined
    ? { name: "unknown", group: "cant", raw: cause }
    : { name: key as CauseName, group, raw: cause };
}

/** The cause of a Not protected protection; null when protected or not known. */
export function notProtectedCause(protection: Protection | undefined | null): Cause | null {
  if (protection === undefined || protection === null || !("not_protected" in protection))
    return null;
  const np = protection.not_protected;
  return np === undefined ? null : causeOf(np.cause);
}

/** Whether a matter stands Not protected (the marks show). */
export const notProtected = (protection: Protection | undefined | null): boolean =>
  notProtectedCause(protection) !== null;
