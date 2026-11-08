import { Tooltip as BaseTooltip } from "@base-ui/react/tooltip";
import { useCallback, useState, type ReactElement, type ReactNode } from "react";
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
  fixed = false,
  within,
  anchor,
}: {
  content: ReactNode;
  /** The trigger: one element that can take focus. */
  children: ReactElement;
  side?: "top" | "bottom" | "right";
  /** Placed against the window, not the page: for a trigger inside a box that scrolls and clips (the wall), where a
   * tooltip placed in the box would be cut off at its edge. It still mounts in the page's layer. */
  fixed?: boolean;
  /** A selector for the trigger's ancestor the tooltip must stay inside: with no room on its side there, it flips to
   * the other. For a trigger in a box that something else covers past its edge, such as the wall under the room's
   * header. The window, when left out. */
  within?: string;
  /** What the tooltip is placed against instead of the trigger: a whole row, so it covers none of the row's
   * neighbours. */
  anchor?: Element | undefined;
}) {
  const [boundary, setBoundary] = useState<Element | null>(null);
  const trigger = useCallback(
    (el: Element | null) =>
      setBoundary(within === undefined ? null : (el?.closest(within) ?? null)),
    [within],
  );
  // it mounts in the page's layer, inside the main landmark, as menus do, and stands in the floating layer, as the
  // floating cards do: over the work surface and the folded rail's panel, which would cut it off
  const layer = useLayer();
  return (
    <BaseTooltip.Root>
      <BaseTooltip.Trigger ref={trigger} render={children} delay={500} />
      <BaseTooltip.Portal container={layer}>
        <BaseTooltip.Positioner
          side={side}
          sideOffset={8}
          collisionPadding={8}
          positionMethod={fixed ? "fixed" : "absolute"}
          className="z-30"
          {...(boundary === null ? {} : { collisionBoundary: boundary })}
          anchor={anchor}
        >
          <BaseTooltip.Popup className={tip}>{content}</BaseTooltip.Popup>
        </BaseTooltip.Positioner>
      </BaseTooltip.Portal>
    </BaseTooltip.Root>
  );
}
