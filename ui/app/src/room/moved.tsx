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
import { useSyncExternalStore } from "react";
import type { Matters } from "./matters.ts";
import { locateRefusalLine } from "./locate-refusal.ts";

// Locate's step for a folder Claude can't start in, which the room mounts once beside these (locate-warning-host.tsx).
export { LocateWarningHost } from "./locate-warning-host.tsx";

/** The moved folder's line: that it moved, where it was (when the host says, H20's folder_display), that nothing is
 * lost, and Locate folder. A Locate the host refused says why under it (`refused`, locate-refusal.ts), in New matter's
 * line for the code: the same primary-tone line (matter-form-error) a refused folder gets there. */
export function MovedFolder({
  folder,
  refused,
  onLocate,
}: {
  folder?: string;
  refused?: string;
  onLocate: () => void;
}) {
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
      {refused !== undefined && (
        <p role="alert" className="text-small-row text-primary" data-testid="locate-refused">
          <Say>{refused}</Say>
        </p>
      )}
      <Button kind="tone" className="room-moved-act" onClick={onLocate}>
        <IconLocateFolder size={16} />
        {ui(lang, "Locate folder")}
      </Button>
    </div>
  );
}

/** The open matter's moved folder line, with the refusal of its last Locate (locate-refusal.ts) said under it. */
export function LocatedFolder({ matters, onLocate }: { matters: Matters; onLocate: () => void }) {
  const { lang } = useWords();
  const refused = useSyncExternalStore(
    matters.locateRefused.subscribe,
    matters.locateRefused.view,
    matters.locateRefused.view,
  );
  const view = matters.view();
  const open = view.open;
  if (open === null) return null;
  const said = refused.get(open.matter);
  return (
    <MovedFolder
      {...(open.folder_display === undefined ? {} : { folder: open.folder_display })}
      {...(said === undefined ? {} : { refused: locateRefusalLine(lang, said, view.matters) })}
      onLocate={onLocate}
    />
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
