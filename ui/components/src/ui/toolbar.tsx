import { Toolbar as BaseToolbar } from "@base-ui/react/toolbar";
import type { ComponentProps, ReactNode } from "react";
import { cn } from "../lib/cn.ts";
import { button, pair, pairButton } from "./recipes.ts";

/** A row of tools, as the room's toolbar: Tab enters and leaves it, the arrow keys move along it. Narrower than its
 * tools, it wraps them onto a second line rather than run past its box; the room's toolbar, which folds its search
 * instead, sets flex-nowrap. */
export function Toolbar({
  label,
  children,
  className,
}: {
  label: string;
  children: ReactNode;
  className?: string;
}) {
  return (
    <BaseToolbar.Root
      aria-label={label}
      className={cn("flex flex-wrap items-center gap-s3 min-w-0", className)}
    >
      {children}
    </BaseToolbar.Root>
  );
}

type ToolbarButtonProps = Omit<ComponentProps<typeof BaseToolbar.Button>, "className"> & {
  className?: string;
};

/** A tool: any kind of button, kept in the arrow-key order even while disabled. */
export function ToolbarButton({
  kind = "quiet",
  size = "default",
  shape = "text",
  className,
  ...props
}: ToolbarButtonProps & {
  kind?: "primary" | "tone" | "quiet";
  size?: "small" | "default" | "large";
  shape?: "text" | "icon";
}) {
  return <BaseToolbar.Button className={cn(button({ kind, size, shape }), className)} {...props} />;
}

/** The joined pair inside a toolbar: History | Undo. */
export function ToolbarPair({
  label,
  children,
  className,
}: {
  label: string;
  children: ReactNode;
  className?: string;
}) {
  return (
    <BaseToolbar.Group aria-label={label} className={cn(pair, className)}>
      {children}
    </BaseToolbar.Group>
  );
}

/** One side of the pair inside a toolbar. */
export function ToolbarPairButton({ className, ...props }: ToolbarButtonProps) {
  return <BaseToolbar.Button className={cn(pairButton, className)} {...props} />;
}

/** The toolbar's input, for the search field to render through. */
export const ToolbarInput = BaseToolbar.Input;
