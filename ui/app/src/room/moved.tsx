// matters/moved (design/03-system/src/pages/matters/room.tsx, other-rooms.tsx): a matter whose folder isn't where it
// was (H3, matters.ts's folderMissing). Both live in the work column, so they are parts U3's Work renders: MovedFolder
// in the thread's place, and LocateGate as the composer's gate, in place of the field. Locate folder is matters.ts's
// locate: the picker, then relocate_matter. "Nothing is lost while the folder is away" holds with H3, whose store is in
// the app's data folder.
import "./moved.css";
import { Button } from "@clauseo/components/ui/button.tsx";
import { IconLocateFolder } from "@clauseo/components/icons/icons.tsx";
import { Say } from "@clauseo/components/clauseo/say.tsx";
import { isolate } from "@clauseo/components/lib/bidi.ts";
import { mt } from "@clauseo/components/lib/words-matters.ts";
import { ui } from "@clauseo/components/lib/words-ui.ts";
import { useWords } from "@clauseo/components/lib/words.ts";

/** The moved folder's line: that it moved, where it was (when the host says, H20's folder_display), that nothing is
 * lost, and Locate folder. */
export function MovedFolder({ folder, onLocate }: { folder?: string; onLocate: () => void }) {
  const { lang } = useWords();
  return (
    <div className="room-moved" role="status">
      <p className="text-ui font-chip text-primary">
        <Say>{mt(lang, "This matter's folder has moved.")}</Say>
      </p>
      {folder !== undefined && folder !== "" && (
        <p className="text-small text-quiet">
          <Say>
            {mt(
              lang,
              "It was at {path}. Find it where it is now, and the matter opens as you left it.",
              // isolated, so a folder name with right-to-left marks can't reorder the line
              { path: isolate(folder) },
            )}
          </Say>
        </p>
      )}
      <p className="text-small text-quiet">
        <Say>
          {mt(
            lang,
            "Its conversations and versions are kept on this computer, so nothing is lost while the folder is away.",
          )}
        </Say>
      </p>
      <Button kind="tone" className="room-moved-act" onClick={onLocate}>
        <IconLocateFolder size={16} />
        {ui(lang, "Locate folder")}
      </Button>
    </div>
  );
}

/** The composer's gate while the folder is away: why it can't ask, and Locate folder. */
export function LocateGate({ onLocate }: { onLocate: () => void }) {
  const { lang } = useWords();
  return (
    <div className="room-gate">
      <span className="text-ui text-muted">
        <Say>{mt(lang, "Locate the folder to ask about this matter")}</Say>
      </span>
      <Button kind="tone" onClick={onLocate}>
        <IconLocateFolder size={16} />
        {ui(lang, "Locate folder")}
      </Button>
    </div>
  );
}
