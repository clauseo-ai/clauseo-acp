// Which of an open group's rows can show what came back (U17), and from where: a finished call with no content but a
// raw output shows its raw output (codex-acp's {result, error}); one whose content holds a picture shows its content
// (claude-agent-acp sends an MCP result's blocks there, a screenshot as an image block, and its rawOutput in
// Anthropic's shape). A pending or running call shows nothing yet, so a result never flickers in mid-run; a thought
// and a mode switch are no results. In the thread's chunk, with the press; everything that reads the output waits in
// result.tsx's chunk.
import type { TextOrBlob, ToolCallContent, ToolCallEntry } from "../../../transport/protocol.ts";

/** What the row shows: its raw output (and whether the call took a screenshot, so a blob past 64 KB is read for its
 * picture though its preview doesn't show one), or its content blocks, which hold a picture. */
export type Result = { raw: TextOrBlob; screenshot: boolean } | { blocks: ToolCallContent[] };

/** agent-browser's screenshot tools, as codex-acp's rawInput names them ({server, tool, arguments}). */
const SCREENSHOT = /"tool":"agent_browser_(?:diff_)?screenshot"/;

/** Whether a content item is a picture (an image block); a newer Clauseo's item is a plain string. */
export const isPicture = (item: ToolCallContent): boolean =>
  typeof item === "object" && item.content?.content?.image !== undefined;

/** What the row shows, undefined when it offers no "Show the result". */
export function resultOf(call: ToolCallEntry): Result | undefined {
  if (call.status !== "completed" && call.status !== "failed" && call.status !== "cancelled")
    return undefined;
  if (call.kind === "think" || call.kind === "switch_mode") return undefined;
  if (call.content.some(isPicture)) return { blocks: call.content };
  if (call.content.length > 0 || call.raw_output === undefined) return undefined;
  const input = call.raw_input?.inline;
  return { raw: call.raw_output, screenshot: input !== undefined && SCREENSHOT.test(input) };
}

/** Whether the row offers "Show the result". */
export const eligible = (call: ToolCallEntry): boolean => resultOf(call) !== undefined;
