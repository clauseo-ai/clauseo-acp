// The room probe's ?tools=<page> (probe-main.tsx): the page is Settings › Tools in one of tools-probe.tsx's scenes,
// with the settings window's sheet, in place of the room. Loaded only on such a page; never shipped.
import "../settings.css";
import type { Lang } from "@clauseo/components/lib/display.ts";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { ToolsSceneOver } from "./tools-probe.tsx";
import { toolsPageOf } from "./tools-scenes.ts";

/** Renders the scene ?tools= names into the page's root; false when it names none. */
export function renderToolsProbe(container: HTMLElement, value: string, lang: Lang): boolean {
  const page = toolsPageOf(value);
  if (page === null) return false;
  createRoot(container).render(
    <StrictMode>
      <ToolsSceneOver page={page} lang={lang} />
    </StrictMode>,
  );
  return true;
}
