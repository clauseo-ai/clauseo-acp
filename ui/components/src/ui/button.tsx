import { Button as BaseButton } from "@base-ui/react/button";
import { Children, type ComponentProps, type ReactNode } from "react";
import { cn } from "../lib/cn.ts";
import { button } from "./recipes.ts";

type Kind = "primary" | "tone" | "quiet" | "link";
type Size = "small" | "default" | "large";
type Shape = "text" | "icon";

export type ButtonProps = Omit<ComponentProps<typeof BaseButton>, "className" | "children"> & {
  /** primary is the Ask button, the only dark one; tone is raised in the ground's tone; quiet has no fill until the
   * pointer arrives; link is the ready hint's quiet "Turn to it". */
  kind?: Kind;
  size?: Size;
  /** icon: a square button holding one icon, named by aria-label. */
  shape?: Shape;
  className?: string;
  children?: ReactNode;
};

/** Words in a control sit on the Devanagari nudge where WebKit ignores the face's metrics. */
const words = (children: ReactNode) =>
  Children.map(children, (child) =>
    typeof child === "string" ? <span className="deva-nudge">{child}</span> : child,
  );

/** One family, three kinds, three heights. Every kind presses to 0.98, never under reduced motion. */
export function Button({
  kind,
  size,
  shape,
  className,
  children,
  type = "button",
  ...props
}: ButtonProps) {
  return (
    <BaseButton type={type} className={cn(button({ kind, size, shape }), className)} {...props}>
      {words(children)}
    </BaseButton>
  );
}
