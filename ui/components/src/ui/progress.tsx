import { Progress as BaseProgress } from "@base-ui/react/progress";
import type { ReactNode } from "react";
import { cn } from "../lib/cn.ts";
import { progressFill, progressTrack } from "./recipes-controls.ts";

/** Progress through a known amount, such as the papers read when a matter opens. Determinate only: nothing in
 * Clauseo spins. The words name what it's doing and how far it has got, and the expected time stands at the right;
 * the 4 px track is primary on the inset tone, as the plan meter is. The fill follows the count and never moves
 * at rest. */
export function Progress({
  value,
  max,
  label,
  expected,
  className,
}: {
  value: number;
  max: number;
  /** What it's doing and how far: "Reading papers, 120 of 400". Also its value in words for screen readers. */
  label: string;
  /** When it will be done: "Expected by 4:32 pm". */
  expected?: ReactNode;
  className?: string;
}) {
  return (
    <BaseProgress.Root
      value={value}
      max={max}
      getAriaValueText={() => label}
      className={cn("grid gap-s2 text-small text-muted", className)}
    >
      <div className="flex flex-wrap items-baseline justify-between gap-x-s4">
        <BaseProgress.Label className="text-primary">{label}</BaseProgress.Label>
        {expected !== undefined && <span>{expected}</span>}
      </div>
      <BaseProgress.Track className={progressTrack}>
        <BaseProgress.Indicator className={progressFill} />
      </BaseProgress.Track>
    </BaseProgress.Root>
  );
}
