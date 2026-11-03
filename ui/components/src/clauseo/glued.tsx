// Words that keep an icon with them when they wrap: the icon after them (the step group's chevron, the working
// line's pen) stays on the line of their last word, and the icon before them (the break) on the line of their first,
// so neither ever stands alone at a line's start or end. The words around them wrap freely, in either language.
import type { ReactNode } from "react";
import { headOf, tailOf } from "./glued-words.ts";
import { Say } from "./say.tsx";

/** The words, with `before` held to their first word and `after` to their last (a no-wrap span each), their text
 * unchanged, so a button's or a row's name still reads them whole. */
export function Glued({
  children,
  before,
  after,
}: {
  children: string;
  before?: ReactNode;
  after?: ReactNode;
}) {
  let [first, rest] = before === undefined ? ["", children] : headOf(children);
  let last = "";
  if (after !== undefined) {
    // one word holds both icons: both go in one span
    if (before !== undefined && rest.trim() === "") {
      first += rest;
      rest = "";
    } else [rest, last] = tailOf(rest);
  }
  const both = after !== undefined && before !== undefined && rest === "" && last === "";
  if (both)
    return (
      <span className="glued">
        {before}
        <Say>{first}</Say>
        {after}
      </span>
    );
  return (
    <>
      {before !== undefined && (
        <span className="glued">
          {before}
          {first !== "" && <Say>{first}</Say>}
        </span>
      )}
      {rest !== "" && <Say>{rest}</Say>}
      {after !== undefined && (
        <span className="glued">
          {last !== "" && <Say>{last}</Say>}
          {after}
        </span>
      )}
    </>
  );
}
