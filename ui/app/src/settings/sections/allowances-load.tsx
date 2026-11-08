// The remembered answers' body (R18) in a chunk of its own, loaded when the section opens: a bare
// dynamic import (vite.config.ts's SETTINGS_BARE_CHUNKS), as the Safe workspace's is, so the settings
// window's first load carries only the section's entry.
import { late } from "@clauseo/components/lib/late.ts";
import { lazy, Suspense } from "react";
import type { SectionContext } from "../sections.tsx";

const Body = lazy(() => late(import("./allowances.tsx")).then((m) => ({ default: m.Allowances })));

/** The section's body, once its chunk is in; nothing before. */
export function AllowancesSection({ ctx }: { ctx: SectionContext }) {
  return (
    <Suspense fallback={null}>
      <Body ctx={ctx} />
    </Suspense>
  );
}
