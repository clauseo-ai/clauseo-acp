// The tool line's popover shell: the status line's quiet button in its own words, and the raised plane it opens
// upward, as the plan's select does (plan-trigger.tsx). Its rows are the caller's. Loaded only with the tool line's
// popover chunk (the app's room/tool-popover.tsx), which is why it builds on Base UI's popover here rather than on
// ui/popover.tsx: that one is History's, and sharing it would split a chunk off both.
import { Popover as BasePopover } from "@base-ui/react/popover";
import type { ReactNode } from "react";
import { useLayer } from "../ui/layer-context.ts";
import { raised } from "../ui/recipes-overlays.ts";

export function ToolLinePopover({
  summary,
  children,
}: {
  /** The status line's phrase, the button's words. */
  summary: string;
  children: ReactNode;
}) {
  const layer = useLayer();
  return (
    <BasePopover.Root>
      <BasePopover.Trigger className="room-plan deva-nudge" data-testid="tool-line">
        {summary}
      </BasePopover.Trigger>
      <BasePopover.Portal container={layer}>
        <BasePopover.Positioner
          positionMethod="fixed"
          side="top"
          align="start"
          sideOffset={6}
          collisionPadding={8}
          className="z-20"
        >
          <BasePopover.Popup className={raised({ width: "menu" })} data-testid="tool-popover">
            {children}
          </BasePopover.Popup>
        </BasePopover.Positioner>
      </BasePopover.Portal>
    </BasePopover.Root>
  );
}
