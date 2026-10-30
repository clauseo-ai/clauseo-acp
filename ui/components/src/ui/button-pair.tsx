import { Button as BaseButton } from "@base-ui/react/button";
import type { ComponentProps, ReactNode } from "react";
import { cn } from "../lib/cn.ts";
import { pair, pairButton } from "./recipes.ts";

/** The joined pair, History | Undo: one raised control on the tile tone, divided by a 1 px inset line. */
export function ButtonPair({
  label,
  children,
  className,
}: {
  label: string;
  children: ReactNode;
  className?: string;
}) {
  return (
    <div role="group" aria-label={label} className={cn(pair, className)}>
      {children}
    </div>
  );
}

export type PairButtonProps = Omit<ComponentProps<typeof BaseButton>, "className"> & {
  className?: string;
};

/** One side of the pair: 13 px words beside a 16 px icon in the muted tone. */
export function PairButton({ className, type = "button", ...props }: PairButtonProps) {
  return <BaseButton type={type} className={cn(pairButton, className)} {...props} />;
}
