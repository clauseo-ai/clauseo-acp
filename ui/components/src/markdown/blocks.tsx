import type { ReactNode } from "react";
import { Kbd } from "../ui/kbd.tsx";

/**
 * Every table in an answer, in a box of its own that scrolls sideways when the table is wider than the
 * answer, so a wide schedule never widens the page. Long cells wrap (a long word or URL breaks rather than
 * forcing the column wider than the box).
 */
export function Table({ children }: { children?: ReactNode }) {
  return (
    <div data-clauseo="table" className="max-w-full overflow-x-auto">
      <table className="break-words">{children}</table>
    </div>
  );
}

/** A key the answer names (`<kbd>`): the design system's key cap, read out as part of the text. */
export function Key({ children }: { children?: ReactNode }) {
  return <Kbd announced>{children}</Kbd>;
}
