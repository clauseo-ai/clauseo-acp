// UC: what a cause carries, outside the room's first load: a full pool's counts (never a name or a SID), why its
// pool had no place (R17 PR-E's `why`, and `needs_reset`), and why a computer can't give protection (R17 PR-B's
// `unsupported { why }`: os, windows_arm, not_built). A logon right the firm's policy denies is `blocked {
// logon_right }` (blocked.ts), never a named `set_up_failed`, which stays a plain string.
import type { NotProtectedCause, PoolFullWhy } from "../../transport/protocol.ts";
import type { Cause } from "./causes.ts";

/** The pool's counts, and why no place was free. */
export type PoolCounts = {
  size: number;
  open: number;
  held: number;
  owed: number;
  set_aside: number;
  /** The places only a reset frees; 0 from an older host (and before R17 PR-D). */
  needs_reset: number;
  /** Why no place was free, when the host knows; null when not, or for a value this version doesn't know. */
  why: Exclude<PoolFullWhy, "unknown"> | null;
};

type PoolFull = Extract<NotProtectedCause, { pool_full: unknown }>["pool_full"];
type Unsupported = Extract<NotProtectedCause, { unsupported: unknown }>["unsupported"];

const objectOf = (cause: Cause): Record<string, unknown> | null =>
  typeof cause.raw === "object" && cause.raw !== null
    ? (cause.raw as Record<string, unknown>)
    : null;

/** A full pool's counts; zeros for a count the host left out, null for a why it left out or this version doesn't
 * know. */
export function poolOf(cause: Cause): PoolCounts {
  const p: Partial<PoolFull> =
    cause.name === "pool_full" ? ((objectOf(cause)?.["pool_full"] ?? {}) as PoolFull) : {};
  const n = (v: unknown) => (typeof v === "number" && Number.isFinite(v) && v >= 0 ? v : 0);
  return {
    size: n(p.size),
    open: n(p.open),
    held: n(p.held),
    owed: n(p.owed),
    set_aside: n(p.set_aside),
    needs_reset: n(p.needs_reset),
    why: p.why === "no_free_slot" || p.why === "matter_sessions_full" ? p.why : null,
  };
}

/** Why the computer can't give protection, by R17 PR-B; null for the plain cause. */
export function whyOf(cause: Cause): string | null {
  if (cause.name !== "unsupported") return null;
  const why = (objectOf(cause)?.["unsupported"] as Partial<Unsupported> | undefined)?.why;
  return typeof why === "string" ? why : null;
}
