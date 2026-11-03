// The composer's warning as it shows (limits/limit-warning.tsx decides when): FieldNotice's quiet note, its first
// sentence the window, its second the reset, the host's `resets_at` in a <time>. In the thread's chunk (list.tsx
// re-exports it), where limit-note.tsx already is for the stop's note, with its words (warning-words.ts), so the first
// load carries none of them.
import { FieldNotice } from "@clauseo/components/clauseo/limit-note.tsx";
import { Say } from "@clauseo/components/clauseo/say.tsx";
import { useDisplay } from "@clauseo/components/lib/display.ts";
import { isoOf } from "./clock.ts";
import { warningWords } from "./warning-words.ts";

/** The note for a warning's key (plan-model.ts's warningKey), its time said against `now`, the render's clock. */
export function WarningNote({ warning, now }: { warning: string; now: number }) {
  const { lang } = useDisplay();
  const words = warningWords(lang, warning, now);
  if (words === null) return null;
  const until = words.until;
  return (
    <div className="contents" data-testid="limit-warning">
      <FieldNotice
        lead={words.lead}
        action={
          until === null ? null : (
            <time className="lim-field-lead" dateTime={isoOf(until.at)}>
              <Say>{until.text}</Say>
            </time>
          )
        }
      />
    </div>
  );
}
