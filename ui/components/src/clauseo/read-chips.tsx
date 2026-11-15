// The other matters a conversation may read, as pills over the composer, the papers' pill look (live.css .q-paper):
// "Can read Sharma's files", each with an × that stops it. A closing pill, withdrawn while a turn still runs, says it
// reads until this answer ends, and has no ×.
import { IconFolder, IconNewMatter } from "../icons/icons.tsx";
import { isolate } from "../lib/bidi.ts";
import { useThreadWords } from "../lib/words-thread.ts";
import { Say } from "./say.tsx";

/** Each matter's pill, by name; `onStop` with its id once its × is pressed. A `closing` one has no ×. Nothing with
 * none. */
export function ReadChips({
  matters,
  onStop,
}: {
  matters: readonly { matter: string; name: string; closing?: boolean }[];
  onStop: (matter: string) => void;
}) {
  const w = useThreadWords();
  if (matters.length === 0) return null;
  return (
    <ul
      className="room-reads"
      aria-label={w.th("Other matters this conversation can read")}
      data-testid="read-chips"
    >
      {matters.map((m) => {
        const matter = isolate(m.name);
        return (
          <li
            key={m.matter}
            className="q-paper"
            data-matter={m.matter}
            data-closing={m.closing === true || undefined}
          >
            <IconFolder size={14} className="q-paper-icon" />
            <span className="q-paper-title">
              <Say>
                {m.closing === true
                  ? w.th("Can read {matter}'s files until this answer ends", { matter })
                  : w.th("Can read {matter}'s files", { matter })}
              </Say>
            </span>
            {m.closing !== true && (
              <button
                type="button"
                className="q-paper-remove"
                data-testid="read-chip-stop"
                aria-label={w.th("Stop letting this conversation read {matter}'s files", {
                  matter,
                })}
                onClick={() => onStop(m.matter)}
              >
                <IconNewMatter size={14} className="rotate-45" />
              </button>
            )}
          </li>
        );
      })}
    </ul>
  );
}
