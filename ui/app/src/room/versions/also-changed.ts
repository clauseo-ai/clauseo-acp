// Whether another conversation's answer, running at the same time, also changed a file (15a's
// FileChange.also_changed_by): undoing takes its change away too, so Undo keeps the file unless the lawyer says
// otherwise, and the changed-paper card says so. Apart from state.ts so the thread's card reads it without Versions.
import type { FileChange } from "../../transport/protocol.ts";

export const alsoChanged = (f: FileChange): boolean => (f.also_changed_by?.length ?? 0) > 0;
