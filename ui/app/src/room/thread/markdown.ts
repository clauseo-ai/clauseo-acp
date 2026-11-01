// The Markdown renderer in its own chunk, so the room's first chunk (room/entry.ts, loaded before the first render)
// doesn't carry it. Work starts the import as it mounts (`loadMarkdown`), long before a first answer arrives.
// `LazyMarkdown` reads the one cached import promise with React's use(): once the chunk is in memory the promise is
// tagged fulfilled, so use() returns it at once and an answer's first render is its content, never a fallback commit
// first. React.lazy can't do that: its loader's promise settles a microtask late, so its first render suspends even
// with the chunk loaded (measured: the first answer's frame committed twice), and the only way round it is a
// hand-written synchronous thenable, which oxlint's unicorn/no-thenable refuses. Before the chunk arrives, use()
// suspends to the row's empty Suspense fallback (rows.tsx), and the chunk's arrival is one commit.
import { createElement, use, type ComponentType } from "react";
// a type alone: erased, so this file has no static dependency on the renderer
import type { MarkdownProps } from "@clauseo/components/markdown/markdown.tsx";

type Loaded = ComponentType<MarkdownProps>;
/** The import, tagged as React's use() reads a settled promise. */
type Chunk = Promise<Loaded> & { status?: "fulfilled"; value?: Loaded };

let chunk: Chunk | null = null;

/** The answer as plain text, should the renderer's chunk fail to load: never a blank row, never a crashed room. */
function PlainAnswer({ text }: MarkdownProps) {
  return createElement("p", { className: "whitespace-pre-wrap", dir: "auto" }, text);
}

/** Starts loading the renderer's chunk, once; resolves when it is in memory. */
export function loadMarkdown(): Promise<Loaded> {
  if (chunk !== null) return chunk;
  const loading: Chunk = import("@clauseo/components/markdown/markdown.tsx").then(
    (m): Loaded => m.Markdown,
    (error: unknown): Loaded => {
      console.warn(`the Markdown renderer did not load: ${String(error)}`);
      return PlainAnswer;
    },
  );
  void loading.then((value) => {
    loading.status = "fulfilled";
    loading.value = value;
  });
  chunk = loading;
  return loading;
}

/** `<Markdown text streaming>`, from its own chunk; suspends only while the chunk is still loading. */
export function LazyMarkdown(props: MarkdownProps) {
  return createElement(use(loadMarkdown()), props);
}
