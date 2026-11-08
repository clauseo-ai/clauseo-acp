// UC: what the firm's requirement says for a cause, the first line of the firm's notice (required.tsx) and, while the
// firm requires the Safe workspace, the Not protected note's body in place of the cause's own (note.tsx): a line that
// says Clauseo can't answer here, never that it works without the Safe workspace. Where the repair is the installer
// (design B), it says so; where a set-up with no Windows prompt fixes it, "Set it up once"; a full pool says R17 PR-E's
// sandbox.pool_full words; a build without the Safe workspace says it comes in an update (not_built); every other
// cause says sandbox.required's.
import type { Lang } from "@clauseo/components/lib/display.ts";
import { errorWords } from "@clauseo/components/lib/words-errors.ts";
import {
  POOL_FULL_CODE,
  protectionErrorWords,
  pw,
} from "@clauseo/components/lib/words-protection.ts";
import { needsInstaller, setUpFixes } from "./blocked.ts";
import type { Cause } from "./causes.ts";
import { whyOf } from "./detail.ts";

/** Whether a set-up from here fixes the cause, so the firm's notice offers it. */
export const requiredFixable = (cause: Cause): boolean =>
  !needsInstaller(cause) &&
  (cause.name === "needs_set_up" || cause.name === "set_up_refused" || setUpFixes(cause));

/** The firm's line for a cause: Clauseo can't answer here, and what brings it back. */
export function requiredWords(lang: Lang, cause: Cause): string {
  if (needsInstaller(cause))
    return pw(
      lang,
      "Your firm requires the Safe workspace on this computer, so Clauseo can't answer here until it's set up.",
    );
  if (requiredFixable(cause))
    return pw(
      lang,
      "Your firm requires the Safe workspace on this computer. Set it up once, then ask here.",
    );
  if (cause.group === "pool") return protectionErrorWords(lang, POOL_FULL_CODE);
  if (whyOf(cause) === "not_built")
    return pw(
      lang,
      "Your firm requires the Safe workspace, and this version of Clauseo doesn't have it yet, so Clauseo can't answer here. The Safe workspace comes in an update.",
    );
  return errorWords(lang, "sandbox.required");
}
