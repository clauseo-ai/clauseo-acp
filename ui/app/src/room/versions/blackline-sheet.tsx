// A Word document's comparison on the desk: the host's Word tool marks up its changes (blackline.ts), set as the
// blackline sheet under the two times. While it runs the times stand with a line saying so; if it fails, a line says
// why (with Try again where another try may mend it) over the sheet that says only that the file changed.
import { useEffect, useMemo, useSyncExternalStore, type ReactNode } from "react";
import { MarkedHead, MarkedSheet, MarkLine } from "@clauseo/components/clauseo/version-page.tsx";
import { markedDiffs } from "@clauseo/components/clauseo/versions-diff.ts";
import { useVersionWords } from "@clauseo/components/lib/words-versions.ts";
import { Button } from "@clauseo/components/ui/button.tsx";
import { keyOf, type Blacklines, type MarkAsk } from "./blackline.ts";
import { markFailedLine } from "./words.ts";

export function MarkedDesk({
  blacklines: b,
  ask,
  title,
  then,
  now,
  fallback,
}: {
  blacklines: Blacklines;
  ask: MarkAsk;
  title: string;
  /** "As it was at 3:42 pm today": when the struck words stood. */
  then: string;
  /** "As it is now". */
  now: string;
  /** The sheet that says only that the file changed, shown when the marking up fails. */
  fallback: ReactNode;
}) {
  const w = useVersionWords();
  const key = keyOf(ask);
  // the ask's fields, not its object, decide when to ask again
  const { path, from, to, now: content } = ask;
  useEffect(() => {
    b.ask({ path, from, now: content, ...(to === undefined ? {} : { to }) });
  }, [b, path, from, to, content]);
  useEffect(() => () => b.leave(), [b]);
  const read = () => b.markOf(key);
  const mark = useSyncExternalStore(b.subscribe, read, read);
  const diffs = useMemo(
    () => (mark?.kind === "shown" ? markedDiffs(mark.paragraphs) : null),
    [mark],
  );
  if (mark === undefined || mark.kind === "running")
    return (
      <>
        <MarkedHead then={then} now={now} />
        <MarkLine text={w.v("Marking up the changes…")} />
      </>
    );
  if (mark.kind === "failed")
    return (
      <>
        <MarkLine
          failed
          text={markFailedLine(w.lang, mark.code)}
          {...(mark.retryable
            ? {
                action: (
                  <Button kind="quiet" size="small" onClick={() => b.ask(ask)}>
                    {w.v("Try again")}
                  </Button>
                ),
              }
            : {})}
        />
        {fallback}
      </>
    );
  return (
    <>
      <MarkedHead then={then} now={now} />
      {mark.truncated && (
        <MarkLine text={w.v("This document is very long, so only part of it is shown.")} />
      )}
      <MarkedSheet title={title} diffs={diffs ?? []} />
    </>
  );
}
