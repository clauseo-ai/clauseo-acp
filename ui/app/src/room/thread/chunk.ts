// The open thread in its own chunk: list.tsx, with Thread, the rows, the change note and the Markdown renderer it
// imports, so the room's first chunk (room/entry.ts, loaded before the first render) carries none of them. Work starts
// the import as it mounts (`loadThread`), and the session opens a thread only once it is in (session.ts), so a
// reopened thread's rows never render, settle or page before it.
// `LazyThreadList` reads the one cached import promise with React's use(): once the chunk is in memory the promise is
// tagged fulfilled, so use() returns it at once and the list's first render is its content, never a fallback commit
// first. React.lazy can't do that: its loader's promise settles a microtask late, so its first render suspends even
// with the chunk loaded (measured: a frame committed twice), and the only way round it is a hand-written synchronous
// thenable, which oxlint's unicorn/no-thenable refuses. Before the chunk arrives, use() suspends to the transcript's
// empty Suspense fallback (work.tsx), and the chunk's arrival is one commit.
import { createElement, use, type ComponentType } from "react";
// a type alone: erased, so this file has no static dependency on the list
import type { ThreadListProps } from "./list.tsx";

type Loaded = ComponentType<ThreadListProps>;
/** The import, tagged as React's use() reads a settled promise. */
type Chunk = Promise<Loaded> & { status?: "fulfilled"; value?: Loaded };

let chunk: Chunk | null = null;

/** Should the chunk fail to load: an empty transcript, never a crashed room; the composer still asks. */
function NoThread(): null {
  return null;
}

/** Starts loading the thread's chunk, once; resolves when it is in memory. */
export function loadThread(): Promise<Loaded> {
  if (chunk !== null) return chunk;
  const loading: Chunk = import("./list.tsx").then(
    (m): Loaded => m.ThreadList,
    (error: unknown): Loaded => {
      console.warn(`the thread's chunk did not load: ${String(error)}`);
      return NoThread;
    },
  );
  void loading.then((value) => {
    loading.status = "fulfilled";
    loading.value = value;
  });
  chunk = loading;
  return loading;
}

/** `<ThreadList>`, from its own chunk; suspends only while the chunk is still loading. */
export function LazyThreadList(props: ThreadListProps) {
  return createElement(use(loadThread()), props);
}
