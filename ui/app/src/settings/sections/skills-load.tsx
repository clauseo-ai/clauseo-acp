// Settings › Skills' body (UC-970 to UC-978) in a chunk of its own, loaded when the section opens: a bare dynamic import
// (vite.config.ts's SETTINGS_BARE_CHUNKS), so the settings window's first load carries only the section's entry.
import { late } from "@clauseo/components/lib/late.ts";
import { lazy, Suspense } from "react";
import type { SectionContext } from "../sections.tsx";

const Body = lazy(() => late(import("./skills.tsx")).then((m) => ({ default: m.SkillsSection })));

/** The section's body, once its chunk is in; nothing before. */
export function SkillsSection({ ctx }: { ctx: SectionContext }) {
  return (
    <Suspense fallback={null}>
      <Body ctx={ctx} />
    </Suspense>
  );
}
