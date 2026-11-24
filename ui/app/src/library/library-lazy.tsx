// The place's chunk, loaded the first time Skills and connectors opens.
import { late } from "@clauseo/components/lib/late.ts";
import { lazy, Suspense, type ComponentProps } from "react";

const Lazy = lazy(() => late(import("./library.tsx")).then((m) => ({ default: m.Library })));

export function LibraryLazy(props: ComponentProps<typeof Lazy>) {
  return (
    <Suspense fallback={null}>
      <Lazy {...props} />
    </Suspense>
  );
}
