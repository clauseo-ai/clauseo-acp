// Find's results' chunk (find-results.tsx, with the search, its shaping and its words), loaded once (U16a): the field
// starts it as the pointer comes over it or focus reaches it, so the first word typed finds it in or on its way. Read
// with React's use() on one cached promise, tagged once settled, as the thread's chunk is (thread/chunk.ts): once in
// memory, the results' first render is their content, never a fallback first. Before it arrives, the rail's Suspense
// shows nothing under the field. A load that failed is tried again on the next ask.
import { createElement, use, type ComponentType } from "react";
import { late } from "@clauseo/components/lib/late.ts";
// a type alone: erased, so this file has no static dependency on the results
import type { FindResultsProps } from "./find-results.tsx";

type Loaded = ComponentType<FindResultsProps>;
type Chunk = Promise<Loaded> & { status?: "fulfilled"; value?: Loaded };

let chunk: Chunk | null = null;

/** Should the chunk fail to load: no results under the field, never a crashed rail. */
function NoResults(): null {
  return null;
}

/** Starts loading Find's results, once; resolves when they are in memory. */
export function preloadFind(): Promise<Loaded> {
  if (chunk !== null) return chunk;
  const loading: Chunk = late(import("./find-results.tsx")).then(
    (m): Loaded => m.FindResults,
    (error: unknown): Loaded => {
      console.warn(`Find's chunk did not load: ${String(error)}`);
      if (chunk === loading) chunk = null;
      return NoResults;
    },
  );
  void late(
    loading.then((value) => {
      loading.status = "fulfilled";
      loading.value = value;
    }),
  );
  chunk = loading;
  return loading;
}

/** `<FindResults>`, from its own chunk; suspends only while the chunk is still loading. */
export function LazyFindResults(props: FindResultsProps) {
  return createElement(use(preloadFind()), props);
}
