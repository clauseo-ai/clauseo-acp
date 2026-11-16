// The room probe's ?tools=<scene> (room/probe-main.tsx, `pnpm build:probe` or `vite --mode probe`): the page is the
// settings window open at Settings › Tools over a fake host (tools-fake.ts), in one of tools-scenes.ts's states, with
// the settings window's sheet, in place of the room. The scenes the shots use: empty, list, add-error, edit-saved,
// firm-list, firm-managed, remote (the address form, previewed), empty-import and list-import; the copy review's
// others are in tools-scenes.ts. The page's query sets its display as the room probe's does (theme=night, lang=hi).
// Loaded only on such a page; never shipped.
import "../settings.css";
import type { Lang } from "@clauseo/components/lib/display.ts";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { ToolsSceneOver } from "./tools-probe.tsx";
import { toolsPageOf } from "./tools-scenes.ts";

/** Renders the scene ?tools= names into the page's root, in the query's language; false when it names none. */
export function renderToolsProbe(
  container: HTMLElement,
  value: string,
  query: URLSearchParams,
): boolean {
  const page = toolsPageOf(value);
  if (page === null) return false;
  const lang: Lang = query.get("lang") === "hi" ? "hi" : "en";
  createRoot(container).render(
    <StrictMode>
      <ToolsSceneOver page={page} lang={lang} />
    </StrictMode>,
  );
  return true;
}
