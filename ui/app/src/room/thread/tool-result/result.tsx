// What came back from a tool call, under its row in an open group (U17): the output read by parse.ts, drawn as plain
// text in one element (pre-wrap, dir="auto", inside <bdi>), never HTML and never a link; folded at 12 lines with
// "Show all", cut at 2,000 lines or 64 KB. A blob is read on the press (blob.ts); past 64 KB, or when its read fails,
// one quiet line says it is too large. An error tells the row (onFailed), which then says Failed. Loaded lazily
// (result-load.ts) with its words: nothing here runs before the press.
import { use, useEffect, useMemo, useState, type ReactNode } from "react";
import { Say } from "@clauseo/components/clauseo/say.tsx";
import type { Lang } from "@clauseo/components/lib/display.ts";
import { tr } from "@clauseo/components/lib/words-tool-result.ts";
import type { BlobRef, TextOrBlob } from "../../../transport/protocol.ts";
import { SourcesTransport } from "../sources.ts";
import { readAlready, readBlob, tooLarge } from "./blob.ts";
import { cut, readOutput } from "./parse.ts";

export type ResultBlockProps = {
  /** The block's id, which the row's press controls. */
  id: string;
  output: TextOrBlob;
  lang: Lang;
  /** Called once the output reads as an error. */
  onFailed: (failed: true) => void;
  /** Whether a blob's read has taken past 400 ms; false again once it ends. */
  onReading: (reading: boolean) => void;
};

/** The block: an inline output read at once, a blob's once its read is in. */
export function ResultBlock({ id, output, lang, onFailed, onReading }: ResultBlockProps) {
  return output.blob !== undefined ? (
    <BlobResult id={id} blob={output.blob} lang={lang} onFailed={onFailed} onReading={onReading} />
  ) : (
    <Shown id={id} text={output.inline} lang={lang} onFailed={onFailed} />
  );
}

function Block({
  id,
  face,
  error = false,
  children,
}: {
  id: string;
  face?: "words" | "code" | undefined;
  error?: boolean;
  children?: ReactNode;
}) {
  return (
    <span
      id={id}
      className="step-row-result"
      data-testid="tool-result"
      data-face={face}
      data-error={error ? true : undefined}
    >
      {children}
    </span>
  );
}

const Note = ({ children }: { children: string }) => (
  <span className="step-row-result-note">
    <Say>{children}</Say>
  </span>
);

const TOO_LARGE = "This result is too large to show here.";

/** A blob: read on mount unless it is too large or read already; nothing shows while the read is out. */
function BlobResult({
  id,
  blob,
  lang,
  onFailed,
  onReading,
}: Omit<ResultBlockProps, "output"> & { blob: BlobRef }) {
  const transport = use(SourcesTransport);
  // undefined while the read is out, null when it isn't read
  const [got, setGot] = useState<string | null | undefined>(() =>
    tooLarge(blob) ? null : readAlready(blob),
  );
  useEffect(() => {
    if (got !== undefined) return;
    let live = true;
    // nothing before 400 ms, so a quick read never flashes
    const slow = setTimeout(() => onReading(true), 400);
    const done = (text: string | null) => {
      clearTimeout(slow);
      if (!live) return;
      onReading(false);
      setGot(text);
    };
    readBlob(transport, blob).then(done, () => done(null));
    return () => {
      live = false;
      clearTimeout(slow);
      onReading(false);
    };
  }, [blob, got, transport, onReading]);
  if (got === undefined) return <Block id={id} />;
  if (got === null)
    return (
      <Block id={id}>
        <Note>{tr(lang, TOO_LARGE)}</Note>
      </Block>
    );
  return <Shown id={id} text={got} lang={lang} onFailed={onFailed} />;
}

/** Text read and drawn: folded at 12 lines until "Show all", the cap's line past 2,000 lines or 64 KB. */
function Shown({
  id,
  text,
  lang,
  onFailed,
}: {
  id: string;
  text: string;
  lang: Lang;
  onFailed: (failed: true) => void;
}) {
  const read = useMemo(() => readOutput(text), [text]);
  const shown = useMemo(() => (read === null ? null : cut(read.text)), [read]);
  const [all, setAll] = useState(false);
  const error = read?.error === true;
  useEffect(() => {
    if (error) onFailed(true);
  }, [error, onFailed]);
  if (read === null || shown === null)
    return (
      <Block id={id}>
        <Note>{tr(lang, "Nothing came back.")}</Note>
      </Block>
    );
  const open = all || !shown.long;
  return (
    <Block id={id} face={read.face} error={error}>
      {read.text !== "" && (
        <bdi>
          <span className="step-row-result-text" dir="auto" data-testid="tool-result-text">
            {open ? shown.all : shown.folded}
          </span>
        </bdi>
      )}
      {read.pictures > 0 && (
        <Note>
          {read.pictures === 1
            ? tr(lang, "1 picture isn't shown here.")
            : tr(lang, "{n} pictures aren't shown here.", { n: read.pictures })}
        </Note>
      )}
      {read.others > 0 && (
        <Note>
          {read.others === 1
            ? tr(lang, "1 more item isn't shown here.")
            : tr(lang, "{n} more items aren't shown here.", { n: read.others })}
        </Note>
      )}
      {!open && (
        <button type="button" className="step-row-press" onClick={() => setAll(true)}>
          <Say>{tr(lang, "Show all")}</Say>
        </button>
      )}
      {open && shown.capped && <Note>{tr(lang, "The rest of this result isn't shown here.")}</Note>}
    </Block>
  );
}
