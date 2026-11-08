// Which of an open group's rows can show what came back (U17): a finished call with no content but a raw output. A
// pending or running call shows nothing yet, so a result never flickers in mid-run; a thought and a mode switch are no
// results. In the thread's chunk, with the press; everything that reads the output waits in result.tsx's chunk.
import type { ToolCallEntry } from "../../../transport/protocol.ts";

/** Whether the row offers "Show the result". */
export const eligible = (call: ToolCallEntry): boolean =>
  call.content.length === 0 &&
  call.raw_output !== undefined &&
  (call.status === "completed" || call.status === "failed" || call.status === "cancelled") &&
  call.kind !== "think" &&
  call.kind !== "switch_mode";
