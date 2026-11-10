import { useContext, useState } from "react";
import { TokenLines } from "./highlight/code-block.tsx";
import { useHighlight } from "./highlight/use-highlight.ts";
import { CodeCopyContext, InertLinksContext } from "./host.ts";

/** How long "Copied" stands in Copy's place. */
const COPIED_MS = 2_000;

/**
 * A fenced or indented code block: the `pre` component, given its code's text and language (to-react.ts). It shows
 * plain until the colouring worker's tokens arrive (highlight/), and stays plain while its fence may still be
 * open, for a language outside the worker's eight, and when the worker fails. Inline code is not this
 * component and is never coloured.
 *
 * "The fence has closed" is read as "the block is not the streaming tail": the splitter keeps an open fence in
 * the last block (it runs to the end of the text), so every other block's fence is closed, and the tail's is
 * final once the turn ends. A closed fence that is still the tail colours when the next block starts.
 */
export function CodeBlock({ text = "", lang }: { text?: string; lang?: string | undefined }) {
  const body = text.endsWith("\n") ? text.slice(0, -1) : text;
  const finished = !useContext(InertLinksContext);
  const lines = useHighlight(body, lang ?? "", finished);
  const words = useContext(CodeCopyContext);
  const pre = (
    <pre data-clauseo="code" data-lang={lang} data-highlighted={lines ? "" : undefined}>
      <code>{lines ? <TokenLines lines={lines} /> : body}</code>
    </pre>
  );
  if (words === null) return pre;
  // Copy over the block's corner once its fence has closed: the code is whole by then
  return (
    <div data-clauseo="code-block" className="relative">
      {pre}
      {finished && <CopyCode text={body} words={words} />}
    </div>
  );
}

/** A code block's Copy (D-026): its text on the clipboard as written, and "Copied" for a moment. */
function CopyCode({ text, words }: { text: string; words: (copied: boolean) => string }) {
  const [copied, setCopied] = useState(false);
  return (
    <button
      type="button"
      className="code-copy"
      data-testid="code-copy"
      aria-live="polite"
      onClick={() => {
        if (navigator.clipboard === undefined) return;
        navigator.clipboard.writeText(text).then(
          () => {
            setCopied(true);
            setTimeout(() => setCopied(false), COPIED_MS);
          },
          () => {},
        );
      }}
    >
      {words(copied)}
    </button>
  );
}
