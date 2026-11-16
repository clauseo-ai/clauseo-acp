// Design B: what stops the Safe workspace, in words, and when the repair is the installer. R17 PR-E
// appends two causes to #570's NotProtectedCause: `blocked { reason }`, whose reason is an open set
// of snake_case names, and `other_account` (another Windows account on this computer owns the Safe
// workspace). The words are picked from the protection state's cause, never from a refused turn's
// `sandbox.blocked`. Each reason has its own line; an unknown one, from a newer Clauseo, says
// only that something on this computer stops it. Outside the room's first load: the note's chunk and
// Settings import it. TODO(R17 PR-E):
// bindings.ts lacks both causes on this base, so the reason is read loosely; once PR-E lands, read
// the generated type and keep this table keyed on the same names.
import type { Lang } from "@clauseo/components/lib/display.ts";
import { pw, type ProtectionKey } from "@clauseo/components/lib/words-protection.ts";
import type { ProtectionView } from "../../transport/protocol.ts";
import type { Cause } from "./causes.ts";

/** A reason the lawyer is told something specific about, and `other_account`. */
export type BlockReason =
  | "logon_right"
  | "policy"
  | "tool_missing"
  | "admin_copy_missing"
  | "admin_copy_changed"
  | "admin_copy_unsafe"
  | "other_account";

/** Each reason's line: the lawyer's own step where one exists (set up again, allow it and install
 * again), else what stops it, never who manages it; a missing admin copy's repair (repairLine)
 * follows its line. */
const REASON_WORDS: Record<BlockReason, ProtectionKey> = {
  logon_right:
    "Windows isn't letting the Safe workspace's accounts sign in. Setting up the Safe workspace again usually fixes this.",
  policy: "A Windows policy on this computer blocks the program that sets up the Safe workspace.",
  tool_missing:
    "The program that sets up the Safe workspace is missing, or security software blocked it. Allow Clauseo in your security software, then install Clauseo again.",
  admin_copy_missing: "The Safe workspace wasn't set up when Clauseo was installed.",
  admin_copy_changed:
    "The program that sets up the Safe workspace has changed since Clauseo was installed. Install Clauseo again to restore it.",
  admin_copy_unsafe:
    "The folder that holds the Safe workspace's set-up program isn't limited to administrators, or holds files it shouldn't. Install Clauseo again to restore it.",
  other_account:
    "The Safe workspace on this computer belongs to another Windows account, so Clauseo can't use it from this one.",
};

/** A reason this version doesn't know. */
const UNKNOWN_WORDS: ProtectionKey = "Something on this computer stops the Safe workspace.";

/** The block a cause names: its reason, "unknown" for a reason (or a shape) this version doesn't
 * know, or null when the cause is neither `blocked` nor `other_account`. */
export function blockOf(cause: Cause | null): BlockReason | "unknown" | null {
  if (cause?.name === "other_account") return "other_account";
  if (cause?.name !== "blocked") return null;
  const raw = cause.raw as { blocked?: { reason?: unknown } } | null;
  const reason = typeof raw === "object" && raw !== null ? raw.blocked?.reason : undefined;
  return typeof reason === "string" &&
    reason !== "other_account" &&
    Object.hasOwn(REASON_WORDS, reason)
    ? (reason as BlockReason)
    : "unknown";
}

/** The block's own line, in the language; null when the cause names no block. */
export function blockLine(lang: Lang, cause: Cause | null): string | null {
  const block = blockOf(cause);
  if (block === null) return null;
  return pw(lang, block === "unknown" ? UNKNOWN_WORDS : REASON_WORDS[block]);
}

/** Whether the repair is the installer: only when the admin copy of the set-up program is missing,
 * for any reason (a declined prompt at install, a silent per-user install, the uninstaller's
 * `--remove`). The host reports `blocked { admin_copy_missing }` up front, at first open, whenever
 * the copy is missing, and `needs_set_up` only when the copy is there and verified (after an in-app
 * Remove, say), so a plain `needs_set_up`, a refused or failed set-up, and a set-up re-run (a pool
 * user that needs a reset, a set-up an update made outdated) keep the app's Set up, with the
 * announce line where `retry_asks_admin` says Windows' prompt follows (stage C's PR 2). */
export const needsInstaller = (cause: Cause | null): boolean =>
  blockOf(cause) === "admin_copy_missing";

/** Whether setting up again usually fixes the block, so it offers the same set-up as `needs_set_up`
 * (the same button, `retry_protection`, and the confirm where Windows' prompt follows): only
 * `logon_right`, since stage C's set-up grants "Allow log on locally" itself. Every other block
 * offers no set-up (a missing admin copy has the installer instead). */
export const setUpFixes = (cause: Cause | null): boolean => blockOf(cause) === "logon_right";

/** The updater's word on who manages this install (`UpdateView.state`'s `managed.by`, from
 * desktop's install_kind.rs): "per_machine" for a per-machine install, "policy" where the firm's
 * policy sets `update.mode` to none (which hides the install kind); null for a per-user install. */
export type ManagedBy =
  | "policy"
  | "per_machine"
  | "read_only_location"
  | "package"
  | "unavailable"
  | null;

/** Whether the firm's policy is active, read from what the protection store already holds: the
 * updater's `managed.by` "policy" (`update.mode` none), `sandbox.required`, or
 * `sandbox.allow_unprotected_auto` off (as a policy that can't be read leaves it). A per-machine
 * install alone isn't a firm's: anyone can install for everyone on a computer of their own. Gates
 * "Your firm manages how Clauseo is installed" (repairLine) and "Copy details for IT" outside the
 * firm's requirement (note.tsx, Settings' workspace.tsx). */
export const firmPolicy = (
  view: ProtectionView | null | undefined,
  managedBy: ManagedBy | undefined,
): boolean =>
  managedBy === "policy" ||
  (view !== null && view !== undefined && (view.required || !view.unprotected_auto_allowed));

/** The repair's line: run the installer again, or, while the firm's policy is active, that the firm manages the install. */
export const repairLine = (
  lang: Lang,
  view: ProtectionView | null | undefined,
  managedBy: ManagedBy | undefined,
): string =>
  pw(
    lang,
    firmPolicy(view, managedBy)
      ? "Your firm manages how Clauseo is installed on this computer."
      : "Run the Clauseo installer again to set up the Safe workspace.",
  );
