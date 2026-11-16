// A Word document's comparison on the desk: the host's Word tool marks up its changes (blackline.ts), set as the
// blackline sheet under the two times. While it runs the times stand with a line saying so; if it fails, a line says
// why (with Try again where another try may mend it) over the sheet that says only that the file changed. Shown, a
// line over it for each thing the host says to know: a link whose web address changed, a part not shown, hidden text,
// numbers that may differ, a comment or a text box whose text changed, the tables whose formatting changed; and after
// the body, each footnote, endnote, header or footer that changed, under its heading.
import { useEffect, useMemo, useSyncExternalStore, type ReactNode } from "react";
import { MarkedHead, MarkedSheet, MarkLine } from "@clauseo/components/clauseo/version-page.tsx";
import { markedDiffs } from "@clauseo/components/clauseo/versions-diff.ts";
import { useVersionWords } from "@clauseo/components/lib/words-versions.ts";
import { Button } from "@clauseo/components/ui/button.tsx";
import { keyOf, type Blacklines, type MarkAsk } from "./blackline.ts";
import {
  linkLine,
  markFailedLine,
  noticeLine,
  partHeadings,
  tablesLine,
} from "./blackline-words.ts";

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
  const lang = w.lang;
  const parts = useMemo(() => {
    if (mark?.kind !== "shown") return [];
    const headings = partHeadings(lang, mark.parts, mark.changedTables);
    return mark.parts.map((p, i) => ({
      heading: headings[i] ?? null,
      diffs: markedDiffs(p.paragraphs),
    }));
  }, [mark, lang]);
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
      {mark.changedLinks.map((text, i) => (
        <MarkLine key={`link-${i}`} text={linkLine(w.lang, text)} />
      ))}
      {mark.notices.map((n) => (
        <MarkLine
          key={n}
          text={
            n === "cell_format_changed"
              ? tablesLine(w.lang, mark.changedTables, mark.parts, mark.changedTablesTotal)
              : noticeLine(w.lang, n)
          }
        />
      ))}
      <MarkedSheet title={title} diffs={diffs ?? []} parts={parts} />
    </>
  );
}
