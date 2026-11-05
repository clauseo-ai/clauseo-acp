// An explanation behind an info icon (DESIGN.md §Icons: reasons in a few words, explanations behind an info icon).
// Base UI's Popover, so one control opens on every input: hover after a short delay, a click or a touch press, and
// Enter or Space from the keyboard. Escape, a press outside or a second press closes it, and focus returns.
import { Popover as BasePopover } from "@base-ui/react/popover";
import type { ReactNode } from "react";
import { IconInfo } from "../icons/icons.tsx";
import { cn } from "../lib/cn.ts";
import { useLayer } from "./layer-context.ts";
import { tip } from "./recipes.ts";

/** The hover delay: shorter than a tooltip's 500 ms, as the icon is there only to be read. */
const HOVER_DELAY = 200;

export function InfoTip({
  label,
  children,
  side = "top",
  className,
}: {
  /** The button's accessible name, in the room's words: "More about Brave Search". */
  label: string;
  /** The explanation, a sentence or two of words. */
  children: ReactNode;
  side?: "top" | "bottom";
  className?: string;
}) {
  // it mounts in the page's layer, inside the main landmark, as tooltips and menus do
  const layer = useLayer();
  return (
    <BasePopover.Root>
      <BasePopover.Trigger
        openOnHover
        delay={HOVER_DELAY}
        closeDelay={0}
        aria-label={label}
        className={cn(
          "inline-grid place-items-center flex-none rounded-ref text-muted is-hover:text-primary",
          className,
        )}
      >
        <IconInfo size={14} />
      </BasePopover.Trigger>
      <BasePopover.Portal container={layer}>
        <BasePopover.Positioner side={side} sideOffset={6} collisionPadding={8}>
          <BasePopover.Popup className={tip} aria-label={label}>
            {children}
          </BasePopover.Popup>
        </BasePopover.Positioner>
      </BasePopover.Portal>
    </BasePopover.Root>
  );
}
