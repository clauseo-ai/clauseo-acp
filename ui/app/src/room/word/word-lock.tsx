// Under the change note, when a program holds a file the direct Undo must write: Word's lines when the undo writes one
// Word document (lock.ts), the error map's line for the code otherwise, and Try again. It rides in Versions' chunk
// (versions-chunk.ts), which holds Versions' words: stop-rows.tsx loads it from there on the first lock, so the thread's
// chunk carries none of those words and most notes never load them.
import { IconTryAgain } from "@clauseo/components/icons/icons.tsx";
import { Say } from "@clauseo/components/clauseo/say.tsx";
import { useDisplay } from "@clauseo/components/lib/display.ts";
import { errorWords } from "@clauseo/components/lib/words-errors.ts";
import { vt } from "@clauseo/components/lib/words-versions.ts";
import { Button } from "@clauseo/components/ui/button.tsx";
import { useEffect, useRef } from "react";
import { fileName } from "../versions/timeline.ts";

export function WordLock({
  file,
  code,
  pending,
  onRetry,
  focusOnMount = false,
  testIds,
}: {
  /** The matter-relative path of the Word document held, or null when the room can't say which file it is. */
  file: string | null;
  /** The refusal's code, for the error map's line when no file is named. */
  code: string;
  /** Try again waits, faint but focusable, while its undo is on its way. */
  pending: boolean;
  onRetry: () => void;
  /** As the lines come, Try again takes focus that was in the note or had fallen to the page (the press that brought
   * them took its button away); focus the lawyer moved elsewhere stays there. */
  focusOnMount?: boolean;
  testIds?: { block?: string; retry?: string };
}) {
  const { lang } = useDisplay();
  const block = useRef<HTMLDivElement>(null);
  const retry = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    if (!focusOnMount) return;
    const at = document.activeElement;
    const note = block.current?.parentElement ?? null;
    if (at === null || at === document.body || note?.contains(at) === true) retry.current?.focus();
  }, [focusOnMount]);
  return (
    <div
      ref={block}
      role="alert"
      data-code={code}
      data-testid={testIds?.block}
      className="mt-(--s2) flex max-w-measure min-w-0 flex-col items-start gap-(--s1) wrap-break-word"
    >
      {file === null ? (
        <p className="text-small text-primary">
          <Say>{errorWords(lang, code)}</Say>
        </p>
      ) : (
        <>
          <p className="text-small text-primary">
            <Say>
              {vt(lang, "Close '{file}' in Word to undo the change.", { file: fileName(file) })}
            </Say>
          </p>
          <p className="text-small text-quiet">
            <Say>{vt(lang, "Word holds the file while it is open. Nothing has changed yet.")}</Say>
          </p>
        </>
      )}
      {/* its words line up with the lines above: the small button's inline padding hangs out to the left */}
      <Button
        kind="quiet"
        size="small"
        className="-ml-s3"
        disabled={pending}
        focusableWhenDisabled
        ref={retry}
        data-testid={testIds?.retry}
        onClick={onRetry}
      >
        <IconTryAgain size={16} />
        <span className="deva-nudge">{vt(lang, "Try again")}</span>
      </Button>
    </div>
  );
}
