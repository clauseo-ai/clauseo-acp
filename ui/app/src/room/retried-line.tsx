// The line a conversation made by Retry from here opens with (H13, `matters/retry`): which question it was retried from,
// that the first answer stays where it was, and Open it, back to that conversation. Loaded only when a retried
// conversation opens, off the first paint. Until H13 names the thread it came from, only a retry made in this window
// shows it.
import { IconUndo } from "@clauseo/components/icons/icons.tsx";
import { Say } from "@clauseo/components/clauseo/say.tsx";
import { mt } from "@clauseo/components/lib/words-matters.ts";
import { clock, useWords } from "@clauseo/components/lib/words.ts";
import { Button } from "@clauseo/components/ui/button.tsx";

export function RetriedLine({
  time,
  title,
  onOpen,
}: {
  /** The question's clock time in English, "4:31 pm" (when.ts's clockOf). */
  time: string;
  /** The first conversation's title, in the language. */
  title: string;
  onOpen: () => void;
}) {
  const { lang } = useWords();
  return (
    <p className="flex max-w-measure flex-wrap items-center gap-x-s2 gap-y-s1 text-small-row text-muted">
      <IconUndo size={16} className="flex-none" />
      <span className="min-w-0 flex-1">
        <Say>
          {mt(lang, "Retried from your question at {time}. The first answer stays in '{title}'.", {
            time: clock(lang, time),
            title,
          })}
        </Say>
      </span>
      <Button
        kind="quiet"
        size="small"
        className="flex-none"
        data-testid="retried-open"
        onClick={onOpen}
      >
        <span className="deva-nudge">{mt(lang, "Open it")}</span>
      </Button>
    </p>
  );
}
