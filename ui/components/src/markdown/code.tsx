import type { Element, ElementContent } from "hast";

function textOf(nodes: readonly ElementContent[]): string {
  let text = "";
  for (const node of nodes) {
    if (node.type === "text") text += node.value;
    else if (node.type === "element") text += textOf(node.children);
  }
  return text;
}

/**
 * A code block, plain and uncoloured: the `pre` component, which reads its `code` child from the hast.
 * Colouring comes later, in a Worker; this is what shows until then, and while the fence is still open.
 */
export function CodeBlock({ node }: { node?: Element | undefined }) {
  const code = node?.children.find(
    (child): child is Element => child.type === "element" && child.tagName === "code",
  );
  const classes = code?.properties["className"];
  const language = Array.isArray(classes)
    ? classes.map(String).find((name) => name.startsWith("language-"))
    : undefined;
  const text = textOf(code ? code.children : []);
  return (
    <pre data-clauseo="code" data-lang={language?.slice("language-".length)}>
      <code>{text.endsWith("\n") ? text.slice(0, -1) : text}</code>
    </pre>
  );
}
