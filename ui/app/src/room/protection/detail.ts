// UC: what a cause carries, outside the room's first load: a full pool's counts (never a name or a SID), why a
// computer can't give protection (R17 PR-B's `unsupported { why }`: os, windows_arm, not_built; null from an older
// host), and R17 PR-E's named cause inside `set_up_failed` for Windows error 1385, whatever its exact shape.
import type { Cause } from "./causes.ts";

/** The pool's counts. */
export type PoolCounts = {
  size: number;
  open: number;
  held: number;
  owed: number;
  set_aside: number;
};

const inner = (cause: Cause): unknown =>
  typeof cause.raw === "object" && cause.raw !== null
    ? (cause.raw as Record<string, unknown>)[cause.name]
    : undefined;

/** A full pool's counts; zeros for a count the host left out. */
export function poolOf(cause: Cause): PoolCounts {
  const p = (inner(cause) ?? {}) as Partial<Record<keyof PoolCounts, unknown>>;
  const n = (v: unknown) => (typeof v === "number" && v >= 0 ? v : 0);
  return {
    size: n(p.size),
    open: n(p.open),
    held: n(p.held),
    owed: n(p.owed),
    set_aside: n(p.set_aside),
  };
}

/** Why the computer can't give protection, by R17 PR-B; null for the plain cause. */
export function whyOf(cause: Cause): string | null {
  const why = (inner(cause) as { why?: unknown } | undefined)?.why;
  return cause.name === "unsupported" && typeof why === "string" ? why : null;
}

/** Whether a failed set-up names Windows error 1385 (a logon right the firm's policy denies), by R17 PR-E. */
export const named1385 = (cause: Cause): boolean =>
  cause.name === "set_up_failed" && /1385|logon/i.test(JSON.stringify(inner(cause) ?? ""));
