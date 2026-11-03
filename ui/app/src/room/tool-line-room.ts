// What the room gives the tool line (tool-line.tsx), and the status line's tool segment for a thread's tools: the
// phrase it says, and the line itself. The phrase is in the first paint; the popover is not.
import { createContext, createElement } from "react";
import type { ToolsSegment } from "@clauseo/components/clauseo/composer.tsx";
import type { Lang } from "@clauseo/components/lib/display.ts";
import { toolsLine } from "@clauseo/components/lib/words-tools-line.ts";
import type { BrowserInstallStore } from "./browser-install.ts";
import { ToolLine } from "./tool-line.tsx";
import { toolsSummary, type KnownTool } from "./tools.ts";

/** What the room gives the tool line: the plan's name, as the status line names it ("Claude"), Settings where the
 * host has the window, and the app's Chrome download. */
export type ToolLineRoom = {
  plan: string;
  openSettings: (() => void) | undefined;
  install: BrowserInstallStore | null;
};

export const ToolLineContext = createContext<ToolLineRoom>({
  plan: "",
  openSettings: undefined,
  install: null,
});

/** The status line's tool segment for these tools; null with none to speak for. */
export function toolsSegment(lang: Lang, tools: readonly KnownTool[]): ToolsSegment | null {
  const key = toolsSummary(tools);
  if (key === null) return null;
  const summary = toolsLine(lang, key);
  return { summary, node: createElement(ToolLine, { summary, tools }) };
}
