// UC: the room's line under the composer, in the room's first load. A protected matter says what the Safe workspace
// does, with the lock, and isn't pressable; a matter without protection carries the Not protected mark, in words true
// for the plan the lawyer talks to: Claude changes files in the matter without asking; ChatGPT reads any file the lawyer
// can open and changes files in the folder without asking (QC.6).
// Protection switched off says the same as any other cause: the
// host runs each plan there as it runs it without protection. A press opens the note that explains the cause. A matter whose protection isn't known shows no line (QU.1).
import { NotProtectedMark } from "@clauseo/components/clauseo/not-protected.tsx";
import type { Lang } from "@clauseo/components/lib/display.ts";
import { tbt } from "@clauseo/components/lib/words-toolbar.ts";
import type { ReactNode } from "react";
import type { Protection } from "../../transport/protocol.ts";
import { notProtectedCause } from "./causes.ts";

/** The plan's kind, as agent.ts's kindOf gives it; undefined for an agent of no kind. */
export type PlanKind = "claude" | "codex" | undefined;

/** What Not protected means for this plan. A plan not known takes ChatGPT's line, which promises the least asking. */
function meaningOf(lang: Lang, plan: PlanKind): string {
  return plan === "claude"
    ? tbt(lang, "Clauseo changes files in this matter without asking.")
    : tbt(
        lang,
        "Clauseo can read any file you can open, and changes files in this matter without asking. It asks before changing files outside it.",
      );
}

/** The line's contents for the composer's line slot: null for the composer's own Safe workspace line, false while
 * the matter's protection isn't known. */
export function protectionLine(
  lang: Lang,
  protection: Protection | undefined,
  plan: PlanKind,
  onPress: () => void,
): ReactNode | false {
  if (protection === undefined) return false;
  const cause = notProtectedCause(protection);
  // protected: null, so the composer says its own Safe workspace line (composer.tsx), the words the room ships
  if (cause === null) return null;
  const meaning = meaningOf(lang, plan);
  return (
    <button
      type="button"
      className="ws-line"
      data-testid="workspace-line"
      data-protection="not_protected"
      data-cause={cause.name}
      aria-haspopup="dialog"
      onClick={onPress}
    >
      <NotProtectedMark meaning={meaning} />
    </button>
  );
}
