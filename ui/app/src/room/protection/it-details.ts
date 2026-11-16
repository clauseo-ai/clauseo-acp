// "Copy details for IT" on the Safe workspace's firm-only lines: the note each surface copies
// (words-first-launch.ts's itNote says its shape and what it may hold). Shown only while the firm's
// policy is active: the firm's requirement (required.tsx), or firmPolicy (blocked.ts) where the Not
// protected note and Settings ask, so a lawyer without a firm is never sent to IT. Only the blocks
// the lawyer can't fix and the firm can offer it: a Windows policy, a reason from a newer Clauseo,
// the logon right and another Windows account. tool_missing and the admin copy's blocks get self-fixes from
// stage C, and a full pool has its own ways out, so none offers it. Each builder reads only the
// cause's names (the reason, why a computer can't) and Clauseo's version: a cause carries no path
// from a matter, and the ProtectionView carries no program path or hash yet, so the note names
// neither. Each note ends with its case's section of Clauseo for firm IT (firmItLink): the
// reason's own where it has one, else the code's.
import type { Lang } from "@clauseo/components/lib/display.ts";
import { firmItLink, itNote } from "@clauseo/components/lib/words-first-launch.ts";
import { BLOCKED_CODE, pw } from "@clauseo/components/lib/words-protection.ts";
import { APP_VERSION } from "../../settings/version.ts";
import { blockOf } from "./blocked.ts";
import type { Cause } from "./causes.ts";
import { whyOf } from "./detail.ts";

const REQUIRED_CODE = "sandbox.required";

/** The blocks only the firm can act on. */
const FIRM_ONLY = new Set(["policy", "unknown", "logon_right", "other_account"]);

/** A host's name for a thing: snake_case, short. Anything else from the wire isn't copied. */
const NAME = /^[a-z][a-z0-9_]{0,63}$/;

/** The reason to report for a firm-only block: its name, or for a reason this version doesn't know,
 * the name the host gave (when it reads as one); null when the block isn't firm-only. */
function firmReason(cause: Cause | null): string | null {
  const block = blockOf(cause);
  if (block === null || !FIRM_ONLY.has(block)) return null;
  if (block !== "unknown") return block;
  const raw = (cause?.raw as { blocked?: { reason?: unknown } } | null)?.blocked?.reason;
  return typeof raw === "string" && NAME.test(raw) ? raw : "unknown";
}

/** The note's section of Clauseo for firm IT: the reason's own where it has one, else the code's;
 * a reason from a newer Clauseo takes the set-up's (firmItLink). */
const linkOf = (reason: string | null | undefined, code?: string): string =>
  firmItLink(reason ?? code ?? "sandbox.blocked");

const blockedLead = (lang: Lang) =>
  pw(lang, "Clauseo's Safe workspace is blocked on this computer.");

export const itDetails = {
  /** Whether a cause's block offers Copy details for IT. */
  offered: (cause: Cause | null): boolean => firmReason(cause) !== null,

  /** A block's line (the Not protected note, Settings): null when the block isn't firm-only. */
  blocked(lang: Lang, cause: Cause | null, version: string = APP_VERSION): string | null {
    const reason = firmReason(cause);
    return reason === null
      ? null
      : itNote(lang, blockedLead(lang), { reason, version, link: linkOf(reason) });
  },

  /** The firm's notice where it requires the Safe workspace (sandbox.required): a firm-only block, or
   * a computer that can't provide it. Null where the lawyer has a step (a set-up, the installer, a
   * pool's ways out, an update) or the block isn't firm-only. */
  required(lang: Lang, cause: Cause, version: string = APP_VERSION): string | null {
    if (blockOf(cause) !== null) {
      const reason = firmReason(cause);
      return reason === null
        ? null
        : itNote(lang, blockedLead(lang), {
            code: REQUIRED_CODE,
            reason,
            version,
            link: linkOf(reason),
          });
    }
    const why = whyOf(cause);
    if (cause.group === "setup" || cause.group === "pool" || why === "not_built") return null;
    const reason = why ?? (cause.name === "unknown" ? undefined : cause.name);
    return itNote(
      lang,
      pw(lang, "The firm requires Clauseo's Safe workspace, and this computer can't provide it."),
      { code: REQUIRED_CODE, reason, version, link: firmItLink(REQUIRED_CODE) },
    );
  },

  /** A protection command's refusal: sandbox.blocked, with the computer's firm-only reason when its
   * cause names one; null for any other code. */
  refused(
    lang: Lang,
    code: string | null,
    cause: Cause | null,
    version: string = APP_VERSION,
  ): string | null {
    if (code !== BLOCKED_CODE) return null;
    const reason = firmReason(cause);
    return itNote(lang, blockedLead(lang), {
      code,
      reason: reason ?? undefined,
      version,
      link: linkOf(reason, code),
    });
  },
};
