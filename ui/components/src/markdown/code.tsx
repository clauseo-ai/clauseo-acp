import type { Element, ElementContent } from "hast";
import { useContext } from "react";
import { TokenLines } from "./highlight/code-block.tsx";
import { useHighlight } from "./highlight/use-highlight.ts";
import { InertLinksContext } from "./host.ts";

function textOf(nodes: readonly ElementContent[]): string {
  let text = "";
  for (const node of nodes) {
    if (node.type === "text") text += node.value;
    else if (node.type === "element") text += textOf(node.children);
  }
  return text;
}

/**
 * A fenced or indented code block: the `pre` component, which reads its `code` child from the hast. It shows
 * plain until the colouring worker's tokens arrive (highlight/), and stays plain while its fence may still be
 * open, for a language outside the worker's eight, and when the worker fails. Inline code is not this
 * component and is never coloured.
 *
 * "The fence has closed" is read as "the block is not the streaming tail": the splitter keeps an open fence in
 * the last block (it runs to the end of the text), so every other block's fence is closed, and the tail's is
 * final once the turn ends. A closed fence that is still the tail colours when the next block starts.
 */
export function CodeBlock({ node }: { node?: Element | undefined }) {
  const code = node?.children.find(
    (child): child is Element => child.type === "element" && child.tagName === "code",
  );
  const classes = code?.properties["className"];
  const language = Array.isArray(classes)
    ? classes.map(String).find((name) => name.startsWith("language-"))
    : undefined;
  const lang = language?.slice("language-".length);
  const text = textOf(code ? code.children : []);
  const body = text.endsWith("\n") ? text.slice(0, -1) : text;
  const finished = !useContext(InertLinksContext);
  const lines = useHighlight(body, lang ?? "", finished);
  return (
    <pre data-clauseo="code" data-lang={lang} data-highlighted={lines ? "" : undefined}>
      <code>{lines ? <TokenLines lines={lines} /> : body}</code>
    </pre>
  );
}
