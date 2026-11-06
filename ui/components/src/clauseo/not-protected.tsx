// The Not protected mark: the open lock and its words, beside a matter's name in the toolbar and in what the matter
// allows (allowed.tsx). A file of its own, with words of its own (words-toolbar.ts), so the toolbar brings neither
// allowed.tsx nor Versions' words into the room's first load.
import { IconNotProtected } from "../icons/icons.tsx";
import { useToolbarWords } from "../lib/words-toolbar.ts";
import { Say } from "./say.tsx";

/** Not protected, as a matter carries it for as long as it lasts, everywhere in the app: the open lock, "Not
 * protected." at chip weight in primary, then what it means in the muted tone, all in concrete and never red. On the
 * room's line it is the way into what the matter allows. The rail and the toolbar carry the short form, the lock and
 * "Not protected" alone. First launch's line, "Not protected." with Try again and Settings, takes this same mark. */
export function NotProtectedMark({
  short = false,
  meaning,
  onPress,
}: {
  short?: boolean;
  /** Optional: a press explains it (UC's note, from the toolbar); the mark is then a button. */
  onPress?: (() => void) | undefined;
  /** What Not protected means where it isn't protection switched off. When the workspace couldn't
   * be set up, or Windows was told no, Clauseo falls back to asking before each change, and the caller says so in its
   * own words: "Clauseo can read any file on this computer and asks before each change." Without it, the sentence about protection switched
   * off stands. */
  meaning?: string;
}) {
  const w = useToolbarWords();
  const Tag = onPress === undefined ? "span" : "button";
  return (
    <Tag
      className="np-mark"
      data-short={short ? "" : undefined}
      {...(onPress === undefined
        ? {}
        : {
            type: "button" as const,
            onClick: onPress,
            "aria-haspopup": "dialog" as const,
            "data-testid": "protection-mark",
          })}
    >
      <IconNotProtected size={14} className="np-icon" />
      <span className="np-words">
        <b className="np-name">{short ? w.v("Not protected") : w.v("Not protected.")}</b>
        {!short && (
          <>
            {" "}
            <Say>
              {meaning ?? w.v("Clauseo can reach beyond this matter's folder without asking.")}
            </Say>
          </>
        )}
      </span>
    </Tag>
  );
}
