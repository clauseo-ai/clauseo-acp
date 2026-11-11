// Settings › Tools' body (UC-169 to UC-189) in a chunk of its own, loaded when the section opens: a bare dynamic import
// (vite.config.ts's SETTINGS_BARE_CHUNKS), so the settings window's first load carries only the section's entry, and
// the list, the form, the confirm and their words load with the section.
import { late } from "@clauseo/components/lib/late.ts";
import { lazy, Suspense } from "react";
import type { SectionContext } from "../sections.tsx";

const Body = lazy(() => late(import("./mcp-tools.tsx")).then((m) => ({ default: m.McpTools })));

/** The section's body, once its chunk is in; nothing before. */
export function ToolsSection({ ctx }: { ctx: SectionContext }) {
  return (
    <Suspense fallback={null}>
      <Body ctx={ctx} />
    </Suspense>
  );
}
