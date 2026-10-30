import { PreviewCard as BasePreviewCard } from "@base-ui/react/preview-card";
import type { ComponentProps, ReactNode } from "react";
import { cn } from "../lib/cn.ts";
import { floating } from "./recipes.ts";

/** What stands behind a reference, on request: the base of the excerpt card. Pointing opens it after 90 ms and
 * leaving closes it after 120 ms. A reference never navigates. */
export const PreviewCard = BasePreviewCard.Root;

type TriggerProps = Omit<ComponentProps<typeof BasePreviewCard.Trigger>, "className"> & {
  /** How the paper is used: the reference's 2 px underline takes the role's colour; a citation keeps the quiet line. */
  role?: "relied" | "supports" | "against" | "cite";
  className?: string;
};

const refLine = {
  relied: "decoration-2 decoration-relied is-hover:bg-hover-faint data-popup-open:bg-hover-faint",
  supports:
    "decoration-2 decoration-green is-hover:bg-supports-ref data-popup-open:bg-supports-ref",
  against: "decoration-2 decoration-red is-hover:bg-against-ref data-popup-open:bg-against-ref",
  cite: "decoration-1 decoration-ref-line is-hover:decoration-primary data-popup-open:decoration-primary",
} as const;

/** A reference in the answer's words: underlined, padded so its tap target grows without moving the lines. */
export function PreviewCardTrigger({ role = "relied", className, ...props }: TriggerProps) {
  return (
    <BasePreviewCard.Trigger
      delay={90}
      closeDelay={120}
      className={cn(
        "underline underline-offset-5 rounded-ref py-s1 motion-colour cursor-default",
        refLine[role],
        className,
      )}
      {...props}
    />
  );
}

/** The card: 320 px from a tile, 348 from a citation row, 368 against a reference; 14 px corners, the surface tone. */
export function PreviewCardPopup({
  width = "ref",
  from = "text",
  side = "bottom",
  align = "start",
  className,
  children,
}: {
  width?: "tile" | "row" | "ref";
  from?: "wall" | "text";
  side?: "top" | "bottom" | "left" | "right";
  align?: "start" | "center" | "end";
  className?: string;
  children: ReactNode;
}) {
  return (
    <BasePreviewCard.Portal>
      <BasePreviewCard.Positioner side={side} align={align} sideOffset={8} collisionPadding={8}>
        <BasePreviewCard.Popup className={cn(floating({ width, from }), "max-w-full", className)}>
          {children}
        </BasePreviewCard.Popup>
      </BasePreviewCard.Positioner>
    </BasePreviewCard.Portal>
  );
}
