// For the tests that read a view's hast: once its update has ended a view keeps its React elements, and its
// hast is made again from its input when read (view.ts). shownHast reads it and checks that it is what the
// view renders, so a suite that checks the hast checks what users see.
import type { Root as Hast } from "hast";
import { expect } from "vitest";
import type { BlockView } from "./view.ts";
import { toReact } from "./to-react.ts";

/** `view`'s hast, checked against its React elements (`content`). */
export function shownHast(view: BlockView): Hast {
  const hast = view.hast;
  expect(view.content, view.source).toEqual(toReact(hast, view.continues));
  return hast;
}
