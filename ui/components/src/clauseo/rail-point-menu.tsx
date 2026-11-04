// A conversation's context menu on the rail (U16b), loaded later: a menu opened at the point the context menu was
// asked for (the pointer, or the row under the focus from the context key), holding the row's items.
import { Menu as BaseMenu } from "@base-ui/react/menu";
import type { ReactNode } from "react";
import { useLayer } from "../ui/layer-context.ts";
import { raised } from "../ui/recipes-overlays.ts";

export function PointMenu({
  at,
  onClose,
  children,
}: {
  at: { x: number; y: number };
  onClose: () => void;
  children: ReactNode;
}) {
  const layer = useLayer();
  const point = {
    getBoundingClientRect: () => DOMRect.fromRect({ x: at.x, y: at.y, width: 0, height: 0 }),
  };
  return (
    <BaseMenu.Root open modal={false} onOpenChange={(open) => !open && onClose()}>
      <BaseMenu.Portal container={layer}>
        <BaseMenu.Positioner
          anchor={point}
          side="bottom"
          align="start"
          collisionPadding={8}
          className="z-20"
        >
          <BaseMenu.Popup className={raised({ width: "menu" })}>{children}</BaseMenu.Popup>
        </BaseMenu.Positioner>
      </BaseMenu.Portal>
    </BaseMenu.Root>
  );
}
