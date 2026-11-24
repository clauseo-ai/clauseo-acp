// Whether another conversation's answer, running at the same time, also changed a file (15a's
// FileChange.also_changed_by): undoing takes its change away too, so Undo keeps the file unless the lawyer says
// otherwise. Apart from state.ts; the thread card keeps its own copy (stop-rows.tsx), sharing no chunk with Versions.
import type { FileChange } from "../../transport/protocol.ts";

export const alsoChanged = (f: FileChange): boolean => (f.also_changed_by?.length ?? 0) > 0;
