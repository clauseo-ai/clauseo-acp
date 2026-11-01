import type { ReactNode } from "react";
import { cn } from "../lib/cn.ts";
import { kbd } from "./recipes.ts";

/** A key cap, in the page's own face rather than the browser's monospace: the arrow key hint beside "Turn to it" and
 * "Back to the answer", 18 px tall, at least 20 wide, drawn by one inner hairline in the muted tone (a 1 px outline
 * in lite). Hidden from assistive technology: the control it sits in names the key with aria-keyshortcuts. Hidden on
 * a narrow window by the part that holds it. `announced` is for a key named in running text (an answer's
 * `<kbd>`), which is part of what a screen reader reads. */
export function Kbd({
  children,
  className,
  announced = false,
}: {
  children: ReactNode;
  className?: string;
  announced?: boolean;
}) {
  return (
    <kbd
      className={cn(kbd, "kbd-cap font-room", className)}
      aria-hidden={announced ? undefined : "true"}
    >
      {children}
    </kbd>
  );
}
