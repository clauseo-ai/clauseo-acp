// The tool line's chunk, loaded once an open thread's header has tools (tool-line-load.ts): what the line has of the
// room, the header's tools it speaks for, and the status line's tool segment for them: the glyphs, and the words that
// name them. The popover is a chunk of its own (tool-popover.tsx).
import { createElement } from "react";
import type { ToolsSegment } from "@clauseo/components/clauseo/composer.tsx";
import type { Lang, Platform } from "@clauseo/components/lib/display.ts";
import type { StreamView } from "../store/view.ts";
import type { BrowserPolicyStore } from "./browser-policy.ts";
import { ToolLine } from "./tool-line.tsx";
import { knownTools, sameTools, toolsLabel, type KnownTool } from "./tools.ts";

/** What the room gives the tool line: the plan's name, as the status line names it ("Claude"), Settings where the
 * host has the window, and the app's re-read of the firm's browser policy. */
export type ToolLineRoom = {
  plan: string;
  openSettings: (() => void) | undefined;
  policy: BrowserPolicyStore | null;
};

let last: readonly KnownTool[] = [];

/** The header's tools the tool line speaks for, the same array for as long as they read the same. */
export function toolsPick(v: StreamView | undefined): readonly KnownTool[] {
  const next = knownTools(v?.header?.header?.tools);
  if (!sameTools(last, next)) last = next;
  return last;
}

/** The status line's tool segment for these tools; null with none to speak for. Its summary is the line's name, so
 * the status line measures its fit again when a tool's state changes. */
export function toolsSegment(
  lang: Lang,
  platform: Platform,
  tools: readonly KnownTool[],
): ToolsSegment | null {
  const label = toolsLabel(lang, platform, tools);
  if (label === null) return null;
  return { summary: label, node: createElement(ToolLine, { label, tools }) };
}
