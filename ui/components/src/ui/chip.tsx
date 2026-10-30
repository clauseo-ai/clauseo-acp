import type { ReactNode } from "react";
import { cn } from "../lib/cn.ts";
import { useWords, type Key } from "../lib/words.ts";
import { IconChecking, IconGoodLaw } from "../icons/icons.tsx";
import { chip } from "./recipes.ts";

/** What the chip says. Solid: a paper's role. Quiet: a stance or a check. */
export type ChipRole =
  | "relied"
  | "supports"
  | "against"
  | "prepared"
  | "accepted"
  | "for"
  | "cite"
  | "good"
  | "checking";

const LABEL: Record<ChipRole, Key> = {
  relied: "Relied on",
  supports: "Supports",
  against: "Against",
  prepared: "Prepared",
  accepted: "Accepted",
  for: "For us",
  cite: "They will cite",
  good: "Good law",
  checking: "Checking",
};

/** The one chip, for every label on the screen. On a lit tile, and in its card's band, it takes the tile's own
 * colour, deeper and opaque, so its words hold whatever the tile behind is doing. Checking carries the wait icon;
 * Good law can carry its tick. */
export function Chip({
  role,
  ground = "surface",
  icon = role === "checking" ? "wait" : "none",
  className,
  children,
}: {
  role: ChipRole;
  ground?: "surface" | "tile";
  icon?: "none" | "wait" | "tick";
  className?: string;
  /** Words other than the role's own, such as a count. */
  children?: ReactNode;
}) {
  const w = useWords();
  return (
    <span className={cn(chip({ role, ground }), className)}>
      {icon === "wait" && <IconChecking size={11} />}
      {icon === "tick" && <IconGoodLaw size={11} />}
      <span className="cap-trim">{children ?? w.t(LABEL[role])}</span>
    </span>
  );
}
