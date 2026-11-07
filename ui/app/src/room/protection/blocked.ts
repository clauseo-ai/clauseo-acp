// Design B: what stops the Safe workspace, in words, and when the repair is the installer. R17 PR-E
// appends two causes to #570's NotProtectedCause: `blocked { reason }`, whose reason is an open set
// of snake_case names, and `other_account` (another Windows account on this computer owns the Safe
// workspace). The words are picked from the protection state's cause, never from a refused turn's
// `sandbox.blocked`. Each reason has its own line; an unknown one, from a newer Clauseo, says to
// ask IT. Outside the room's first load: the note's chunk and Settings import it. TODO(R17 PR-E):
// bindings.ts lacks both causes on this base, so the reason is read loosely; once PR-E lands, read
// the generated type and keep this table keyed on the same names.
import type { Lang } from "@clauseo/components/lib/display.ts";
import { pw, type ProtectionKey } from "@clauseo/components/lib/words-protection.ts";
import type { ProtectionView } from "../../transport/protocol.ts";
import type { Cause } from "./causes.ts";
import { asksAdmin } from "./view.ts";

/** A reason the lawyer is told something specific about, and `other_account`. */
export type BlockReason =
  | "logon_right"
  | "policy"
  | "tool_missing"
  | "admin_copy_missing"
  | "admin_copy_changed"
  | "admin_copy_unsafe"
  | "other_account";

/** Each reason's line: ask IT, except a missing admin copy, whose repair (repairLine) follows it. */
const REASON_WORDS: Record<BlockReason, ProtectionKey> = {
  logon_right:
    "Windows policy on this computer doesn't let the Safe workspace's accounts sign in. Ask your IT team to add them to the Allow log on locally policy.",
  policy:
    "A Windows policy on this computer blocks the program that sets up the Safe workspace. Ask your IT team to allow it.",
  tool_missing:
    "The program that sets up the Safe workspace is missing, or security software blocked it. Ask your IT team to look into it.",
  admin_copy_missing: "The Safe workspace wasn't set up when Clauseo was installed.",
  admin_copy_changed:
    "The program that sets up the Safe workspace has changed since Clauseo was installed. Ask your IT team, or install Clauseo again.",
  admin_copy_unsafe:
    "The folder that holds the Safe workspace's set-up program isn't limited to administrators, or holds files it shouldn't. Ask your IT team to look into it.",
  other_account:
    "The Safe workspace on this computer belongs to another Windows account. Ask your IT team.",
};

/** A reason this version doesn't know. */
const UNKNOWN_WORDS: ProtectionKey = "Ask your IT team to look into it.";

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

/** Whether the repair is the installer: the admin copy is missing, or (from an older host) a set-up
 * cause whose retry would bring Windows' prompt, which design B's app never raises for a set-up. */
export function needsInstaller(
  view: ProtectionView | null | undefined,
  cause: Cause | null,
): boolean {
  if (cause === null) return false;
  if (blockOf(cause) === "admin_copy_missing") return true;
  return cause.group === "setup" && asksAdmin(view, cause);
}

/** A firm install, as the protection state can tell: the firm's policy moved either of its two
 * protection settings off its default (`sandbox.required` false, `sandbox.allow_unprotected_auto`
 * true). */
export const firmInstall = (view: ProtectionView | null | undefined): boolean =>
  view !== null && view !== undefined && (view.required || !view.unprotected_auto_allowed);

/** The repair's line: run the installer again (a personal install), or ask IT to (a firm's). */
export const repairLine = (lang: Lang, view: ProtectionView | null | undefined): string =>
  pw(
    lang,
    firmInstall(view)
      ? "Ask your IT team to install Clauseo again."
      : "Run the Clauseo installer again to set up the Safe workspace.",
  );
