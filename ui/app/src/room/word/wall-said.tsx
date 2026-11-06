// What a tile's open said, under the wall's tiles (paper-wall.tsx), as the change note says it under the note: a quiet
// line when Word opened the file in Protected View, or, for a path too long for Word, the error line with Show in
// folder beside it, which shows the matter's folder; nothing for a tile's other outcomes, whose failures go only to the
// console. It rides in the thread's chunk (thread/list.tsx), with the change note's lines, so the room's first load
// carries none of it.
import { Say } from "@clauseo/components/clauseo/say.tsx";
import { useDisplay } from "@clauseo/components/lib/display.ts";
import { errorWords } from "@clauseo/components/lib/words-errors.ts";
import { revealKey, ui } from "@clauseo/components/lib/words-ui.ts";
import { Button } from "@clauseo/components/ui/button.tsx";
import type { WordOpened, WordOpener } from "./open-in-word.ts";

const PATH_TOO_LONG = "matter.document_path_too_long";

/** The line under the tiles, as the change note's: the space above it on the spacing tokens. */
const QUIET = "mt-(--s3) text-small text-quiet";

export function WallSaid({ opener, opened }: { opener: WordOpener; opened: WordOpened }) {
  const { lang, platform } = useDisplay();
  if (opened.refused === PATH_TOO_LONG)
    return (
      <div className={QUIET}>
        <p role="alert" data-code={PATH_TOO_LONG} data-testid="wall-error">
          <Say>{errorWords(lang, PATH_TOO_LONG)}</Say>
        </p>
        <Button kind="quiet" size="small" onClick={opener.onReveal} data-testid="reveal">
          <span className="deva-nudge">{ui(lang, revealKey(platform))}</span>
        </Button>
      </div>
    );
  if (opened.refused !== null || !opened.protectedView) return null;
  return (
    <p className={QUIET} role="status" data-testid="open-protected-view">
      <Say>{opener.protectedLine}</Say>
    </p>
  );
}
