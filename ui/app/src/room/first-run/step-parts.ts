// UC: the first run's Safe workspace step, as first-run.tsx draws it: the parts it takes from the
// note's chunk, its words, and design B's repair (blocked.ts, installer.ts).
import type { Display, Lang } from "@clauseo/components/lib/display.ts";
import { fl } from "@clauseo/components/lib/words-first-launch.ts";
import type { ProtectionState } from "../../store/protection.ts";
import type { loadProtectionNote } from "../protection/note-load.ts";

/** UC: the Safe workspace step's parts, from the note's chunk (note-load.ts), so the two share no
 * chunk of their own. */
export type StepParts = Pick<
  Awaited<ReturnType<typeof loadProtectionNote>>,
  | "WorkspaceOff"
  | "WorkspaceStep"
  | "failedWords"
  | "retryProtection"
  | "pw"
  | "asksAdmin"
  | "inProgress"
  | "computerCause"
  | "blockOf"
  | "blockLine"
  | "needsInstaller"
  | "firmInstall"
  | "repairLine"
  | "repairPage"
  | "openInstallerPage"
>;

/** The step's words: the first run's own, and protection's from the chunk. */
export const stepWords = (lang: Lang, pw: StepParts["pw"]) => ({
  ask: fl(lang, "Windows asks once more for permission, to finish setting up the Safe workspace."),
  unknown: pw(
    lang,
    "Windows will ask to let Clauseo finish setting up the Safe workspace. It will say the publisher is unknown, because this early version of Clauseo isn't signed yet. Choose Yes. If Windows asks for an administrator's password, whoever looks after this computer can enter it.",
  ),
  setUp: pw(lang, "Set up the Safe workspace"),
  waiting: pw(lang, "Choose Yes in the window Windows shows."),
  notNow: fl(lang, "Not now"),
  wait: pw(lang, "Setting up the Safe workspace."),
  tryAgain: fl(lang, "Try again"),
  settings: fl(lang, "Settings"),
  getInstaller: pw(lang, "Get the installer"),
});

/** UC, design B: the repair's lines (the cause's, then what to do) and its page, null for no
 * button: a firm install, a dry run, or a version with no release page. */
export function repairOf(
  state: ProtectionState,
  parts: StepParts,
  lang: Lang,
  display: Pick<Display, "releaseMode">,
) {
  const cause = parts.computerCause(state.view);
  const block = parts.blockLine(lang, cause);
  return {
    lines: [
      ...(block === null ? [] : [block]),
      parts.repairLine(lang, state.view, state.managedBy),
    ],
    page: parts.firmInstall(state.view, state.managedBy) ? null : parts.repairPage(display),
  };
}
