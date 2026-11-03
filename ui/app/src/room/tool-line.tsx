// The tool line on the status line, after the plan: the open conversation's tools in one phrase, a quiet button that
// opens where each stands. The popover, its rows, the Chrome download and their words are a chunk of their own
// (tool-popover.tsx), loaded as the line first shows. Until it is in, the phrase stands as the status line's own
// words, as the button does at rest.
import { lazy, Suspense, use } from "react";
import { late } from "@clauseo/components/lib/late.ts";
import { ToolLineContext } from "./tool-line-room.ts";
import type { KnownTool } from "./tools.ts";

const ToolPopover = lazy(() =>
  late(import("./tool-popover.tsx")).then((m) => ({ default: m.ToolPopover })),
);

export function ToolLine({ summary, tools }: { summary: string; tools: readonly KnownTool[] }) {
  const room = use(ToolLineContext);
  return (
    <Suspense fallback={<span className="room-plan deva-nudge">{summary}</span>}>
      <ToolPopover summary={summary} tools={tools} room={room} />
    </Suspense>
  );
}
