// A card that opens from a control on the room's own layer of floating things. It is the system's
// PopoverCard, the surface tone, 14 px corners and e3, with its positioner standing over the raised work and the
// timeline's held day headings, where a card opened from the rail or the room's line has to stand. Focus stays on
// the control that opened it; Tab goes on into the card.
import { Popover as BasePopover } from "@base-ui/react/popover";
import type { ReactNode } from "react";
import { floating } from "../ui/recipes.ts";
import { cn } from "../lib/cn.ts";

export function FloatingCard({
  side,
  align = "start",
  width = "fit",
  className,
  children,
}: {
  side: "top" | "bottom";
  align?: "start" | "center" | "end";
  width?: "history" | "fit";
  className?: string;
  children: ReactNode;
}) {
  return (
    <BasePopover.Portal>
      <BasePopover.Positioner
        side={side}
        align={align}
        sideOffset={8}
        collisionPadding={8}
        className="z-30"
      >
        <BasePopover.Popup
          className={cn(floating({ width }), "shadow-e3", className)}
          initialFocus={false}
        >
          {children}
        </BasePopover.Popup>
      </BasePopover.Positioner>
    </BasePopover.Portal>
  );
}
