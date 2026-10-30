import { Separator as BaseSeparator } from "@base-ui/react/separator";
import { cn } from "../lib/cn.ts";
import { separator } from "./recipes-controls.ts";

/** The one drawn division, where two groups need parting and space alone won't do: between a menu's groups,
 * between settings rows, between joined controls. 1 px of the divide tone on a whole pixel. It stays in lite, where
 * the planes part by tone and shadows go. Screen readers hear a separator. */
export function Separator({
  orientation = "horizontal",
  className,
}: {
  orientation?: "horizontal" | "vertical";
  className?: string;
}) {
  return (
    <BaseSeparator orientation={orientation} className={cn(separator[orientation], className)} />
  );
}
