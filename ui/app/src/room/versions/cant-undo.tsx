// The thread's change note after an undo (thread/stop-rows.tsx): a quiet line for each file the
// undo left as it is because Clauseo had no earlier copy of it, in Versions' words (words.ts's
// cantUndoWords). In Versions' chunk, which holds those words: the thread loads it only for such an
// undo.
import { useDisplay } from "@clauseo/components/lib/display.ts";
import { Say } from "@clauseo/components/clauseo/say.tsx";
import type { FolderPlace, SkippedFile } from "../../transport/protocol.ts";
import { cantUndoWords } from "./words.ts";

export function CantUndo({
  files,
  place,
  testId,
}: {
  files: readonly SkippedFile[];
  /** Where the matter's folder lives: its sync service names where an earlier copy may be. */
  place: FolderPlace | undefined;
  testId?: string;
}) {
  const { lang } = useDisplay();
  return files.map((f) => (
    <p key={f.path} className="text-small text-quiet" role="status" data-testid={testId}>
      <Say>{cantUndoWords(lang, f.path, f.reason, place)}</Say>
    </p>
  ));
}
