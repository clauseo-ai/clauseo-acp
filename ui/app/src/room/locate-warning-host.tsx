// Where Locate's step for a folder Claude can't start in shows (locate-held.ts): mounted once in the room, it tells the
// store the OS while it is mounted and shows the step for a pick held, loading the step and its words the first time
// one is held, so the first paint carries neither.
import { late } from "@clauseo/components/lib/late.ts";
import { useDisplay } from "@clauseo/components/lib/display.ts";
import { lazy, Suspense, useEffect, useSyncExternalStore } from "react";
import type { LocateHeld } from "./locate-held.ts";

const Step = lazy(() =>
  late(import("./locate-warning.tsx")).then((m) => ({ default: m.LocateWarning })),
);

export function LocateWarningHost({ held }: { held: LocateHeld }) {
  const { platform } = useDisplay();
  useEffect(() => {
    held.shown(platform);
    return () => held.shown(null);
  }, [held, platform]);
  const now = useSyncExternalStore(held.subscribe, held.current, held.current);
  if (now === null) return null;
  return (
    <Suspense fallback={null}>
      <Step held={now} onClose={() => held.drop()} />
    </Suspense>
  );
}
