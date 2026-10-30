// Heading levels. A part that names itself with a heading takes the level from where it stands, so the same part
// reads right as the room's own section and inside another page's outline. Without a
// HeadingLevel around it, each part keeps the level it has in the room.
import { createContext, use, type HTMLAttributes, type ReactNode, type Ref } from "react";

/** A heading level, or none: the words stay, styled as before, but stand outside the page's outline. */
export type Level = 1 | 2 | 3 | 4 | 5 | 6 | "none";

const LevelContext = createContext<Level | null>(null);

/** Sets the level the headings of the parts inside start at. A part's sub-headings go one deeper. */
export function HeadingLevel({ level, children }: { level: Level; children: ReactNode }) {
  return <LevelContext value={level}>{children}</LevelContext>;
}

type HeadingProps = HTMLAttributes<HTMLHeadingElement> & {
  /** The level this heading has in the room, used where no HeadingLevel says otherwise. */
  level: 1 | 2 | 3 | 4 | 5 | 6;
  /** How far below the part's own heading this one stands: 0 for the part's name, 1 for its groups. */
  depth?: number;
  ref?: Ref<HTMLHeadingElement>;
};

/** A heading at the level its place gives it. */
export function Heading({ level, depth = 0, ref, ...props }: HeadingProps) {
  const base = use(LevelContext);
  if (base === "none") return <p {...props} ref={ref} />;
  const n = Math.min(6, Math.max(1, (base ?? level) + depth));
  const Tag =
    n === 1 ? "h1" : n === 2 ? "h2" : n === 3 ? "h3" : n === 4 ? "h4" : n === 5 ? "h5" : "h6";
  return <Tag {...props} ref={ref} />;
}
