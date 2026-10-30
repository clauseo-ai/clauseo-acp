import { Tooltip as BaseTooltip } from "@base-ui/react/tooltip";
import type { ReactElement, ReactNode } from "react";
import { useLayer } from "./layer-context.ts";
import { tip } from "./recipes.ts";

/** Wraps the page once: tooltips open after 500 ms, and a neighbour opens at once while one shows. */
export function TooltipProvider({ children }: { children: ReactNode }) {
  return (
    <BaseTooltip.Provider delay={500} closeDelay={0}>
      {children}
    </BaseTooltip.Provider>
  );
}

/** A hint for sighted users on what a control or heading is. It never holds the only copy of a name: the
 * trigger's own words or aria-label carry that. It fades in and rises 4 px over 160 ms, and leaves in 90. */
export function Tooltip({
  content,
  children,
  side = "top",
}: {
  content: ReactNode;
  /** The trigger: one element that can take focus. */
  children: ReactElement;
  side?: "top" | "bottom";
}) {
  // it mounts in the page's layer, inside the main landmark, as menus do
  const layer = useLayer();
  return (
    <BaseTooltip.Root>
      <BaseTooltip.Trigger render={children} delay={500} />
      <BaseTooltip.Portal container={layer}>
        <BaseTooltip.Positioner side={side} sideOffset={8} collisionPadding={8}>
          <BaseTooltip.Popup className={tip}>{content}</BaseTooltip.Popup>
        </BaseTooltip.Positioner>
      </BaseTooltip.Portal>
    </BaseTooltip.Root>
  );
}
