// A code mode script that failed (stage B's #1320): the host keys on Claude Code's fixed sentence and sends a notice
// of kind "script_failed", level info, its text the host's English fallback; Claude's raw sentence never reaches the
// log. The app shows one quiet line inside the step group, in its own words (UC-1246), never counted and never
// splitting the group. Read here only. Pure.
import type { NoticeEntry } from "../../transport/protocol.ts";

/** Whether a notice is a code mode script's failure. NoticeKind takes any string until #1320's bindings add
 * "script_failed" to it, so this reads the same before and after. */
export const scriptFailedOf = (notice: NoticeEntry | undefined): boolean =>
  notice?.kind === "script_failed";
