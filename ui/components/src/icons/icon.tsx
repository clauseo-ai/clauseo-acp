import type { ReactNode } from "react";
import { cn } from "../lib/cn.ts";

export type IconProps = {
  /** Drawn size in px on the 20 unit grid: 16 in buttons and the toolbar, 18 in a card's icon tile, 14 for the
   * room's line, the accept mark and Open, 13 in a fold (12 for its chevron, as the room draws it), 11 in a chip. */
  size?: 11 | 12 | 13 | 14 | 16 | 18 | 20;
  /** Names the icon for assistive technology, in the room's words. Without it the icon is hidden, and the words
   * beside it name it. */
  label?: string;
  className?: string;
};

/** One icon: a 1.5 px stroke whatever the size, square ends, sharp joins, in the current colour. */
export function Icon({
  size = 16,
  label,
  className,
  children,
}: IconProps & { children: ReactNode }) {
  const named =
    label === undefined ? { "aria-hidden": true } : { role: "img", "aria-label": label };
  return (
    <svg
      viewBox="0 0 20 20"
      width={size}
      height={size}
      fill="none"
      stroke="currentColor"
      strokeWidth={1.5}
      strokeLinecap="square"
      strokeLinejoin="miter"
      strokeMiterlimit={4}
      focusable="false"
      className={cn("icon", className)}
      {...named}
    >
      {children}
    </svg>
  );
}
