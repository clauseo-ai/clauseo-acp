import { useContext } from "react";
import { CopyButton } from "../ui/copy-button.tsx";
import { TokenLines } from "./highlight/code-block.tsx";
import { useHighlight } from "./highlight/use-highlight.ts";
import { CodeCopyContext, InertLinksContext } from "./host.ts";

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
  // Copy in the block's own column at its top corner (thread.css), beside the code that scrolls and never over it,
  // once its fence has closed: the code is whole by then
  return (
    <div data-clauseo="code-block">
      {pre}
      {finished && (
        <CopyButton
          className="code-copy mx-s1-5 my-s1 self-start"
          testId="code-copy"
          text={() => body}
          label={words(false)}
          done={words(true)}
        />
      )}
    </div>
  );
}
