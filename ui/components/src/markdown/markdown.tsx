// The answer renderer: one memoised block per top-level Markdown block, so finished blocks never re-render
// while the tail streams. The model (answer.ts) does the split, the definitions, the parse and the
// sanitiser; this file only maps each block's sanitised hast to React through our components.
import { memo, useId, useState } from "react";
import { Fragment, jsx, jsxs } from "react/jsx-runtime";
import { toJsxRuntime } from "hast-util-to-jsx-runtime";
import type { Components } from "hast-util-to-jsx-runtime";
import { createAnswer } from "./answer.ts";
import type { BlockView } from "./answer.ts";
import { BlockBoundary } from "./boundary.tsx";
import { CodeBlock } from "./code.tsx";
import { AnswerScopeContext, scopeOf } from "./host.ts";
import { Image } from "./image.tsx";
import { LinkOrReference } from "./link.tsx";

/**
 * AppendText-shaped props: `text` is the whole answer so far and `appended` what the last patch added;
 * `streaming` is false once the turn has ended.
 */
export type MarkdownProps = { text: string; appended?: string; streaming?: boolean };

const COMPONENTS: Partial<Components> = { a: LinkOrReference, img: Image, pre: CodeBlock };
const OPTIONS = { Fragment, jsx, jsxs, components: COMPONENTS, passNode: true };

const Block = memo(function Block({ view }: { view: BlockView }) {
  return (
    <BlockBoundary source={view.source}>
      <BlockContent view={view} />
    </BlockBoundary>
  );
});

function BlockContent({ view }: { view: BlockView }) {
  return toJsxRuntime(view.hast, OPTIONS);
}

export function Markdown({ text, streaming = true }: MarkdownProps) {
  const scope = scopeOf(useId());
  const [answer] = useState(() => createAnswer(scope));
  const blocks = answer.update(text, streaming);
  return (
    <AnswerScopeContext value={scope}>
      <div data-clauseo="markdown">
        {blocks.map((view, index) => (
          <Block key={index} view={view} />
        ))}
      </div>
    </AnswerScopeContext>
  );
}
