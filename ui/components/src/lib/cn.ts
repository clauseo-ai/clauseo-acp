import { cx } from "class-variance-authority";
import { extendTailwindMerge } from "tailwind-merge";
import * as names from "./token-names.ts";

/** tailwind-merge, told that only Clauseo's tokens exist: Tailwind's own theme is never imported, so its default
 * scales mean nothing here, and a class such as text-ui must read as a size, not a colour. */
const merge = extendTailwindMerge<"stretch" | "dur" | "motion">({
  override: {
    theme: {
      color: [...names.color],
      font: [...names.font],
      text: [...names.text],
      "font-weight": [...names.fontWeight],
      tracking: [...names.tracking],
      leading: [...names.leading],
      spacing: [...names.spacing],
      radius: [...names.radius],
      shadow: [...names.shadow],
      ease: [...names.ease],
    },
  },
  extend: {
    classGroups: {
      stretch: [{ stretch: [...names.stretch] }],
      dur: [{ dur: [...names.dur] }],
      motion: [{ motion: ["control", "colour", "turn", "float", "tip", "fold"] }],
    },
  },
});

/** Joins class names, later ones winning where two set the same thing. The only place tailwind-merge is used. */
export function cn(...inputs: Parameters<typeof cx>): string {
  return merge(cx(...inputs));
}
