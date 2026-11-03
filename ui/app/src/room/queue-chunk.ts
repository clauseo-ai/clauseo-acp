// The queue strip's chunk (queue-strip.tsx, with the parts' QueueStrip), loaded apart from the room's first one, as the
// thread's is (thread/chunk.ts, which says why use() on a tagged promise and not React.lazy). The slot starts it as
// it mounts (queue-slot.tsx).
import { createElement, use, type ComponentType } from "react";
import { late } from "@clauseo/components/lib/late.ts";
// a type alone: erased, so this file has no static dependency on the strip
import type { QueueStripProps } from "./queue-slot.tsx";

type Loaded = ComponentType<QueueStripProps>;
type Chunk = Promise<Loaded> & { status?: "fulfilled"; value?: Loaded };

let chunk: Chunk | null = null;

/** Should the chunk fail to load: no strip, never a crashed composer. */
function NoStrip(): null {
  return null;
}

/** Starts loading the strip's chunk, once; resolves when it is in memory. */
export function loadQueueStrip(): Promise<Loaded> {
  if (chunk !== null) return chunk;
  const loading: Chunk = late(import("./queue-strip.tsx")).then(
    (m): Loaded => m.LiveQueueStrip,
    (error: unknown): Loaded => {
      console.warn(`the queue's chunk did not load: ${String(error)}`);
      return NoStrip;
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

/** The strip, from its own chunk; suspends only while the chunk is still loading. */
export function LazyQueueStrip(props: QueueStripProps) {
  return createElement(use(loadQueueStrip()), props);
}
