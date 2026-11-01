// The answer renderer: one memoised block per top-level Markdown block, so finished blocks never re-render
// while the tail streams. The model (answer.ts) does the split, the definitions, the parse and the
// sanitiser; this file only maps each block's sanitised hast to React through our components.
import { memo, useEffect, useId, useState, useSyncExternalStore } from "react";
import { createAnswer } from "./answer.ts";
import type { BlockView } from "./answer.ts";
import { BlockBoundary } from "./boundary.tsx";
import { characterEntitiesLoaded, onCharacterEntities, prefetchEntities } from "./entities.ts";
import { AnswerScopeContext, InertLinksContext, scopeOf } from "./host.ts";
import { Notes } from "./notes.tsx";
import { toReact } from "./to-react.ts";

/**
 * `text` is the whole answer so far (an AppendText patch's result; what the patch appended is not needed,
 * since the model finds the unchanged blocks itself). `streaming` is true while the turn is running. It
 * defaults to false, so a host that leaves it out (a reopened transcript) shows finished text: no tail
 * repair, every link live.
 */
export type MarkdownProps = { text: string; streaming?: boolean };

const Block = memo(function Block({ view }: { view: BlockView }) {
  return (
    <BlockBoundary source={view.source}>
      {view.streaming ? (
        <InertLinksContext value={true}>
          <BlockContent view={view} />
        </InertLinksContext>
      ) : (
        <BlockContent view={view} />
      )}
    </BlockBoundary>
  );
});

/** The notes view: the section, and each note as its own memoised block, so one note changing re-renders it alone. */
const NotesBlock = memo(function NotesBlock({ items }: { items: readonly BlockView[] }) {
  return (
    <Notes>
      {items.map((item) => (
        <Block key={item.note} view={item} />
      ))}
    </Notes>
  );
});

function BlockContent({ view }: { view: BlockView }) {
  return toReact(view.hast);
}

export function Markdown({ text, streaming = false }: MarkdownProps) {
  const scope = scopeOf(useId());
  const [answer] = useState(() => createAnswer(scope));
  // Re-render once the full entity table arrives: the model then re-parses only the blocks that need it.
  useSyncExternalStore(onCharacterEntities, characterEntitiesLoaded, characterEntitiesLoaded);
  useEffect(prefetchEntities, []);
  const blocks = answer.update(text, streaming);
  return (
    <AnswerScopeContext value={scope}>
      <div data-clauseo="markdown">
        {blocks.map((view, index) =>
          view.items ? (
            <NotesBlock key="notes" items={view.items} />
          ) : (
            <Block key={index} view={view} />
          ),
        )}
      </div>
    </AnswerScopeContext>
  );
}
