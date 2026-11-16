// The Safe workspace's body (UC) in a chunk of its own, loaded when the section opens: a bare dynamic import
// (vite.config.ts's SETTINGS_BARE_CHUNKS), so the settings window's first load carries only the section's entry, and
// the protection store, the confirm and their words load with the section.
import { late } from "@clauseo/components/lib/late.ts";
import { lazy, Suspense } from "react";
import type { SectionContext } from "../sections.tsx";

const Body = lazy(() => late(import("./workspace.tsx")).then((m) => ({ default: m.Workspace })));

/** The section's body, once its chunk is in; nothing before. */
export function WorkspaceSection({ ctx }: { ctx: SectionContext }) {
  return (
    <Suspense fallback={null}>
      <Body ctx={ctx} />
    </Suspense>
  );
}
