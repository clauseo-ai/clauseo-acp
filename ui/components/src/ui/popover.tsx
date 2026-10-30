import { Popover as BasePopover } from "@base-ui/react/popover";
import type { ComponentProps, ReactNode } from "react";
import { cn } from "../lib/cn.ts";
import { floating } from "./recipes.ts";

/** A card that opens from a control and stays until dismissed: the base of History. Click, tap or Enter opens it
 * with aria-expanded on the trigger; Escape, a press outside or a second press closes it, and focus returns. */
export const Popover = BasePopover.Root;
export const PopoverTrigger = BasePopover.Trigger;
export const PopoverTitle = BasePopover.Title;

/** The card itself: the surface tone, 14 px corners, e3. It drops in from 4 px above and 0.97 scale over 160 ms,
 * and leaves in 90 ms. Right edge to the trigger's, 8 px below, always inside the window. */
export function PopoverCard({
  width = "history",
  side = "bottom",
  align = "end",
  className,
  children,
  ...props
}: Omit<ComponentProps<typeof BasePopover.Popup>, "className"> & {
  width?: "history" | "fit";
  side?: "top" | "bottom";
  align?: "start" | "center" | "end";
  className?: string;
  children: ReactNode;
}) {
  return (
    <BasePopover.Portal>
      <BasePopover.Positioner side={side} align={align} sideOffset={8} collisionPadding={8}>
        <BasePopover.Popup className={cn(floating({ width }), "shadow-e3", className)} {...props}>
          {children}
        </BasePopover.Popup>
      </BasePopover.Positioner>
    </BasePopover.Portal>
  );
}
