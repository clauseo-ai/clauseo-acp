// What a tile's open said, under the wall's tiles (paper-wall.tsx), as the change note says it
// under the note: a quiet line when Word opened the file in Protected View, or a refused open's
// words, by its code, in the error line: for a path too long for Word with Show in folder beside
// it, which shows the matter's folder, and for a folder gone or not the matter's with Locate folder
// (act-refusals.ts). It rides in the thread's chunk (thread/list.tsx), with the change note's
// lines, so the room's first load carries none of it.
import { Say } from "@clauseo/components/clauseo/say.tsx";
import { useDisplay } from "@clauseo/components/lib/display.ts";
import { errorWords } from "@clauseo/components/lib/words-errors.ts";
import { revealKey, ui } from "@clauseo/components/lib/words-ui.ts";
import { Button } from "@clauseo/components/ui/button.tsx";
import { locatable, useLocateFolder } from "../act-refusals.ts";
import { LocateButton } from "../locate-button.tsx";
import type { WordOpened, WordOpener } from "./open-in-word.ts";

const PATH_TOO_LONG = "matter.document_path_too_long";

/** The line under the tiles, as the change note's: the space above it on the spacing tokens. */
const QUIET = "mt-(--s3) text-small text-quiet";

/** `onRefused`: Show in folder was refused, with this code, which the line says in its place. */
export function WallSaid({
  opener,
  opened,
  onRefused,
}: {
  opener: WordOpener;
  opened: WordOpened;
  onRefused: (code: string) => void;
}) {
  const { lang, platform } = useDisplay();
  const locate = useLocateFolder();
  const code = opened.refused;
  if (code !== null)
    return (
      <div className={QUIET}>
        <p role="alert" data-code={code} data-testid="wall-error">
          <Say>{errorWords(lang, code)}</Say>
        </p>
        {code === PATH_TOO_LONG && (
          <Button
            kind="quiet"
            size="small"
            onClick={() => void opener.onReveal().then((c) => c !== null && onRefused(c))}
            data-testid="reveal"
          >
            <span className="deva-nudge">{ui(lang, revealKey(platform))}</span>
          </Button>
        )}
        {locatable(code) && locate !== null && <LocateButton onLocate={locate} />}
      </div>
    );
  if (!opened.protectedView) return null;
  return (
    <p className={QUIET} role="status" data-testid="open-protected-view">
      <Say>{opener.protectedLine}</Say>
    </p>
  );
}
