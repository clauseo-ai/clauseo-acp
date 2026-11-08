// R18: one remembered answer, as Settings' "Remembered answers" and the room's protection note list
// it: the folder, file or site it covers (isolated, so a right-to-left name can't reorder the
// line), what kind it is (a folder or file allowed is read only), "Allowed" or "Not allowed", and
// Forget. The folder, file or site comes from outside Clauseo, so it is shown through isolate.ts
// (its control and bidi characters dropped, the rest isolated). Spans only, so it sits inside the note's description, a <p>; the caller gives the row's
// frame.
import { useDisplay } from "../lib/display.ts";
import { isolate } from "../lib/isolate.ts";
import {
  aw,
  forgetLabel,
  targetKind,
  targetText,
  type AllowTargetLike,
} from "../lib/words-allowances.ts";
import { settingLabel, settingNote } from "../ui/recipes-controls.ts";
import { Button } from "../ui/button.tsx";
import { Say } from "./say.tsx";

export function RememberedRow({
  target,
  allowed,
  busy = false,
  onForget,
}: {
  /** What it covers. */
  target: AllowTargetLike;
  /** Allowed, or refused. */
  allowed: boolean;
  /** A Forget is with the host: every Forget greys until it answers. */
  busy?: boolean;
  onForget: () => void;
}) {
  const { lang } = useDisplay();
  const text = targetText(target);
  return (
    <>
      <span className="block min-w-0" data-testid="remembered-row">
        {text !== null && (
          <span className={`block ${settingLabel}`}>
            <bdi>{isolate(text)}</bdi>
          </span>
        )}
        <span className={`block ${settingNote}`}>
          <Say>{`${targetKind(lang, target, allowed)} · ${aw(lang, allowed ? "Allowed" : "Not allowed")}`}</Say>
        </span>
      </span>
      <Button
        kind="quiet"
        size="small"
        data-testid="remembered-forget"
        aria-label={forgetLabel(lang, target, allowed)}
        disabled={busy}
        onClick={onForget}
      >
        <span className="deva-nudge">{aw(lang, "Forget")}</span>
      </Button>
    </>
  );
}
