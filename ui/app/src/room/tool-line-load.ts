// The tool line's chunk (tool-line-room.ts), started the first time an open thread's header has tools, and kept: a
// conversation opened after it draws its line at once. Until it is in, and should it fail to load, the status line
// stands as it does without tools.
import { useEffect, useState } from "react";
import type { ToolsSegment } from "@clauseo/components/clauseo/composer.tsx";
import type { Lang } from "@clauseo/components/lib/display.ts";
import { late } from "@clauseo/components/lib/late.ts";
import { useStreamPart } from "../store/hooks.ts";
import type { ViewStore } from "../store/store.ts";
import type { StreamId } from "../transport/protocol.ts";
import type { StreamView } from "../store/view.ts";
import type { KnownTool } from "./tools.ts";

type ToolLineChunk = typeof import("./tool-line-room.ts");

let loaded: ToolLineChunk | null = null;

/** Before the chunk is in: whether the header has any tools, the chunk worth loading. */
const hasTools = (v: StreamView | undefined): boolean =>
  (v?.header?.header?.tools?.length ?? 0) > 0;

/** The status line's tool segment for the stream's header (tool-line-room.ts), once the line's chunk is in; null
 * before, and with no tools to speak for. */
export function useToolsSegment(
  store: ViewStore,
  stream: StreamId | null,
  lang: Lang,
): ToolsSegment | null {
  const [chunk, setChunk] = useState(loaded);
  // the header's tools as the line reads them once its chunk is in, and until then whether it has any
  const tools = useStreamPart<readonly KnownTool[] | boolean>(
    store,
    stream,
    chunk?.toolsPick ?? hasTools,
  );
  useEffect(() => {
    if (tools === true)
      late(import("./tool-line-room.ts")).then(
        (m) => setChunk((loaded = m)),
        () => {},
      );
  }, [tools]);
  return typeof tools === "boolean" || chunk === null ? null : chunk.toolsSegment(lang, tools);
}
