// Windows' Controlled folder access, said in one place (issue 5): the first screen's row (permissions.tsx), the open
// matter's line (app's room/matter-blocked.tsx) and the matters-folder choice (folder-choice.tsx) all read these keys,
// so a change of stage E's advice is one edit. Each is a key of words-first-launch.ts.
import type { FirstLaunchKey } from "../../lib/words-first-launch.ts";

/** Windows' words, in one place (the first screen's row and the matter's line, matter-blocked.tsx): the advice is the action, and the desktop's settings_path and note are
 * extra detail under it, shown as given. A change of advice is an edit here. */
export const WINDOWS = {
  blocking: "Windows is blocking your matters folder",
  unknown: "Windows may be blocking your matters folder",
  /** Stage E's words (issue 5's Windows run): the block falls on the assistants' tools, not on Clauseo. */
  why: "Windows blocks the assistants' tools, such as PowerShell, from saving in protected folders. Choose a matters folder outside Documents, Desktop and Pictures, or ask your IT team.",
  act: "Choose a matters folder outside Documents",
} as const satisfies Record<string, FirstLaunchKey>;
