// What came back from a tool call, under its row in an open group (U17): the output read by parse.ts, drawn as plain
// text in one element (pre-wrap, dir="auto", inside <bdi>), never HTML and never a link; folded at 12 lines with
// "Show all", cut at 2,000 lines or 64 KB. An MCP picture of a raster type (a screenshot) is drawn under the text,
// scaled down to the column and a bounded height, never cut, with an empty alt (it adds no words). Claude's result
// comes as content blocks (eligible.ts): their text and pictures are drawn the same way. A blob is read on the press
// (blob.ts); past 64 KB without a picture, or when its read fails, one quiet line says it is too large. An error tells
// the row (onFailed), which then says Failed. Loaded lazily (result-load.ts) with its words: nothing here runs before
// the press.
import { use, useEffect, useMemo, useState, type ReactNode } from "react";
import { Say } from "@clauseo/components/clauseo/say.tsx";
import type { Lang } from "@clauseo/components/lib/display.ts";
import { tr } from "@clauseo/components/lib/words-tool-result.ts";
import type { BlobRef, TextOrBlob, ToolCallContent } from "../../../transport/protocol.ts";
import type { HostTransport } from "../../../transport/host-transport.ts";
import { SourcesTransport } from "../sources.ts";
import type { Result } from "./eligible.ts";
import { MAX_PICTURE_READ, holdsPicture, readAlready, readBlob, tooLarge } from "./blob.ts";
import { cut, readOutput } from "./parse.ts";

export type ResultBlockProps = {
  /** The block's id, which the row's press controls. */
  id: string;
  /** What the row shows (eligible.ts). */
  result: Result;
  lang: Lang;
  /** Called once the output reads as an error. */
  onFailed: (failed: true) => void;
  /** Whether a blob's read has taken past 400 ms; false again once it ends. */
  onReading: (reading: boolean) => void;
};

type Common = Omit<ResultBlockProps, "result">;

/** The block: an inline output read at once, a blob's once its read is in, content blocks once their blobs are. */
export function ResultBlock({ result, ...common }: ResultBlockProps) {
  if ("blocks" in result) return <ContentResult blocks={result.blocks} {...common} />;
  const output: TextOrBlob = result.raw;
  return output.blob !== undefined ? (
    <BlobResult blob={output.blob} screenshot={result.screenshot} {...common} />
  ) : (
    <Shown id={common.id} text={output.inline} lang={common.lang} onFailed={common.onFailed} />
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

/** Reads on mount unless `first` already says what came back; nothing shows while the read is out. `load` reads the
 * text Shown draws (its first `max` read), or rejects, and the one quiet line says the result is too large. */
function Reading({
  id,
  lang,
  onFailed,
  onReading,
  first,
  load,
  max,
}: Common & {
  /** The text when it is in already, null when it isn't read, undefined to read it. */
  first: () => string | null | undefined;
  load: (transport: HostTransport | null) => Promise<string>;
  max?: number | undefined;
}) {
  const transport = use(SourcesTransport);
  // undefined while the read is out, null when it isn't read
  const [got, setGot] = useState<string | null | undefined>(first);
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
    load(transport).then(done, () => done(null));
    return () => {
      live = false;
      clearTimeout(slow);
      onReading(false);
    };
  }, [load, got, transport, onReading]);
  if (got === undefined) return <Block id={id} />;
  if (got === null)
    return (
      <Block id={id}>
        <Note>{tr(lang, TOO_LARGE)}</Note>
      </Block>
    );
  return <Shown id={id} text={got} max={max} lang={lang} onFailed={onFailed} />;
}

/** A blob: read unless it is too large or read already; a picture's whole. */
function BlobResult({
  blob,
  screenshot,
  ...common
}: Common & { blob: BlobRef; screenshot: boolean }) {
  const whole = holdsPicture(blob, screenshot);
  const load = useMemo(
    () => (transport: HostTransport | null) => readBlob(transport, blob, whole),
    [blob, whole],
  );
  return (
    <Reading
      {...common}
      first={() => (tooLarge(blob, screenshot) ? null : readAlready(blob))}
      load={load}
      max={whole ? MAX_PICTURE_READ : undefined}
    />
  );
}

/** Content blocks (Claude's), drawn as an MCP result of the same blocks: text, and pictures with their data, a blob's
 * read (a text's first 64 KB, a picture's whole up to 8 MB). A picture past 8 MB goes without its data, so it is
 * counted as not shown; any other block is counted as another item. */
function ContentResult({ blocks, ...common }: Common & { blocks: ToolCallContent[] }) {
  const load = useMemo(
    () => async (transport: HostTransport | null) => {
      const read = async (value: TextOrBlob, whole: boolean) =>
        value.blob === undefined ? value.inline : readBlob(transport, value.blob, whole);
      const content = await Promise.all(
        blocks.map(async (item) => {
          if (typeof item !== "object") return { type: "other" };
          if (item.text !== undefined)
            return { type: "text", text: await read(item.text.text, false) };
          const image = item.content?.content?.image;
          if (image === undefined) return { type: "other" };
          const size = image.data.blob?.size_bytes ?? 0;
          if (size > MAX_PICTURE_READ) return { type: "image", mimeType: image.mime_type };
          return { type: "image", data: await read(image.data, true), mimeType: image.mime_type };
        }),
      );
      return JSON.stringify({ content });
    },
    [blocks],
  );
  return <Reading {...common} first={() => undefined} load={load} max={Infinity} />;
}

/** Text read and drawn: folded at 12 lines until "Show all", the cap's line past 2,000 lines or 64 KB. */
function Shown({
  id,
  text,
  max,
  lang,
  onFailed,
}: {
  id: string;
  text: string;
  /** How much of the text is read: MAX_TEXT unless a picture's blob was read whole. */
  max?: number | undefined;
  lang: Lang;
  onFailed: (failed: true) => void;
}) {
  const read = useMemo(() => readOutput(text, max), [text, max]);
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
      {read.images.map((picture, i) => (
        <img
          // the pictures never reorder: their place is their key
          key={i}
          className="step-row-result-image"
          data-testid="tool-result-image"
          src={`data:${picture.mime};base64,${picture.data}`}
          alt=""
          decoding="async"
        />
      ))}
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
