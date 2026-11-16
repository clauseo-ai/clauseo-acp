// Connectors' body (U15) in a chunk of its own, loaded when the section opens: a bare dynamic import (vite.config.ts's
// BARE_CHUNKS), so the settings window's first load carries only the section's entry, its side note and the
// connectors store.
import { late } from "@clauseo/components/lib/late.ts";
import { lazy, Suspense } from "react";
import type { SectionContext } from "../sections.tsx";

const Body = lazy(() => late(import("./connectors.tsx")).then((m) => ({ default: m.Connectors })));

/** The section's body, once its chunk is in; nothing before. */
export function ConnectorsSection({ ctx }: { ctx: SectionContext }) {
  return (
    <Suspense fallback={null}>
      <Body ctx={ctx} />
    </Suspense>
  );
}
