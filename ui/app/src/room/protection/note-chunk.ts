// UC: the chunk note-load.ts loads: the Not protected note, the firm's notice, and UD1's first run's Safe workspace
// step parts (first-run.tsx takes them from here, so the two share no chunk of their own).
export { ProtectionPart } from "./part.tsx";
export {
  WorkspaceOff,
  WorkspaceStep,
} from "@clauseo/components/clauseo/first-launch/workspace.tsx";
export { failedWords } from "./words.ts";
export { retryProtection } from "./commands.ts";
export { asksAdmin, computerCause, inProgress } from "./view.ts";
export { blockLine, blockOf, firmInstall, needsInstaller, repairLine } from "./blocked.ts";
export { openInstallerPage, repairPage } from "./installer.ts";
export { markLanded, repairView } from "./repair.ts";
export { pw } from "@clauseo/components/lib/words-protection.ts";
