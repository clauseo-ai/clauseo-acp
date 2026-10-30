import { Collapsible as BaseCollapsible } from "@base-ui/react/collapsible";
import type { ComponentProps, ReactNode } from "react";
import { cn } from "../lib/cn.ts";
import { IconFold, IconMore } from "../icons/icons.tsx";
import { foldTrigger } from "./recipes.ts";

/** A fold: "N more" at the wall's foot, or an earlier exchange folded to one line that keeps its result in view. */
export const Collapsible = BaseCollapsible.Root;

/** The fold's trigger. more: a 40 px strip in the ground's tone with a chevron down. fold: one quiet line led by a
 * chevron that turns a quarter as it opens. */
export function CollapsibleTrigger({
  kind = "fold",
  className,
  children,
  ...props
}: Omit<ComponentProps<typeof BaseCollapsible.Trigger>, "className" | "children"> & {
  kind?: "more" | "fold";
  className?: string;
  children: ReactNode;
}) {
  return (
    <BaseCollapsible.Trigger className={cn(foldTrigger({ kind }), className)} {...props}>
      {kind === "more" ? (
        <IconMore size={16} className="motion-turn group-data-panel-open:rotate-180" />
      ) : (
        <IconFold size={12} className="motion-turn group-data-panel-open:rotate-90" />
      )}
      {children}
    </BaseCollapsible.Trigger>
  );
}

/** What the fold holds. It opens to its own height over 160 ms; under reduced motion and in lite, at once. */
export function CollapsiblePanel({
  className,
  children,
  ...props
}: Omit<ComponentProps<typeof BaseCollapsible.Panel>, "className" | "children"> & {
  className?: string;
  children: ReactNode;
}) {
  return (
    <BaseCollapsible.Panel className={cn("motion-fold", className)} {...props}>
      {children}
    </BaseCollapsible.Panel>
  );
}
