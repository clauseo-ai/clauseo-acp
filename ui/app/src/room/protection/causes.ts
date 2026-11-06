// UC: what a Protection says, as the room's line, the marks and the firm's hold read it, in the room's first load. The
// wire's NotProtectedCause is open (a newer Clauseo's cause decodes as a plain string), and R17 PR-B and PR-E add
// shapes bindings.ts doesn't have yet: `unsupported { why }`, and a named cause inside `set_up_failed` for Windows
// error 1385. A cause is read here only for its name and group; what it carries (the pool's counts, the why, the
// named cause) is detail.ts's, outside the first load.
import type { NotProtectedCause, Protection } from "../../transport/protocol.ts";

/** The cause groups (UC, Screens): set-up (Try again offered), can't (no retry), the pool, and switched off. */
export type CauseGroup = "setup" | "cant" | "pool" | "off";

/** A cause's data-cause name: the wire's, or "unknown" for a shape this version doesn't know. */
export type CauseName =
  | "needs_set_up"
  | "set_up_refused"
  | "set_up_failed"
  | "unsupported"
  | "unknown"
  | "pool_full"
  | "switched_off";

/** One cause, decoded: its name and group, and the wire's value for detail.ts. */
export type Cause = { name: CauseName; group: CauseGroup; raw: unknown };

const GROUP: Partial<Record<string, CauseGroup>> = {
  needs_set_up: "setup",
  set_up_refused: "setup",
  set_up_failed: "setup",
  unsupported: "cant",
  pool_full: "pool",
  switched_off: "off",
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
  const np = (protection as { not_protected?: { cause: unknown } } | null | undefined)
    ?.not_protected;
  return np === undefined ? null : causeOf(np.cause as NotProtectedCause);
}

/** Whether a matter stands Not protected (the marks show). */
export const notProtected = (protection: Protection | undefined | null): boolean =>
  notProtectedCause(protection) !== null;
