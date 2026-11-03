// What the room gives the tool line (tool-line.tsx), and the status line's tool segment for a thread's tools: the
// glyphs, and the words that name them. Both are in the first paint; the popover is not.
import { createContext, createElement } from "react";
import type { ToolsSegment } from "@clauseo/components/clauseo/composer.tsx";
import type { Lang } from "@clauseo/components/lib/display.ts";
import type { BrowserInstallStore } from "./browser-install.ts";
import { ToolLine } from "./tool-line.tsx";
import { toolsLabel, type KnownTool } from "./tools.ts";

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

/** The status line's tool segment for these tools; null with none to speak for. Its summary is the line's name, so
 * the status line measures its fit again when a tool's state changes. */
export function toolsSegment(lang: Lang, tools: readonly KnownTool[]): ToolsSegment | null {
  const label = toolsLabel(lang, tools);
  if (label === null) return null;
  return { summary: label, node: createElement(ToolLine, { label, tools }) };
}
