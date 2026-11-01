// Limits on nesting, so hostile input cannot overflow the stack. marked lexes block quotes recursively and
// mdast-util-to-hast, the sanitiser and the JSX mapping walk the tree recursively: `"> ".repeat(3000)` or
// `"- ".repeat(1000)`, 2 to 6 KB of agent text, threw "Maximum call stack size exceeded" and unmounted the
// answer on every render. Two layers:
// - Before lexing, a line that opens more than MAX_NESTING block quotes or list items with its leading
//   markers gets its next marker escaped, so the deeper part shows as plain text in the deepest container.
// - After parsing, a block whose tree is deeper than MAX_TREE_DEPTH (nesting built through indentation, or
//   thousands of nested emphasis runs) shows its source as plain text (answer.ts).
import type { Nodes } from "mdast";

/** Block quotes and list items one line may open with its leading markers. */
export const MAX_NESTING = 32;
/** Levels of mdast below a block's root: 32 nested lists are 64 (list and item), plus their inline content. */
export const MAX_TREE_DEPTH = 128;

const THEMATIC_BREAK = /^[ \t]*([-*_])(?:[ \t]*\1){2,}[ \t]*$/;

/** Where to put a backslash in `text[start, end)` (one line) to stop it opening a deeper container, or -1. */
function deepMarker(text: string, start: number, end: number): number {
  let count = 0;
  let at = start;
  while (at < end) {
    const char = text.charCodeAt(at);
    if (char === 32 || char === 9) {
      at++;
      continue;
    }
    let marker = -1;
    let next = at + 1;
    if (char === 62 /* > */) marker = at;
    else if (char === 45 || char === 43 || char === 42 /* - + * */) {
      const after = text.charCodeAt(next);
      if (next === end || after === 32 || after === 9) marker = at;
    } else if (char >= 48 && char <= 57) {
      while (
        next < end &&
        next - at < 9 &&
        text.charCodeAt(next) >= 48 &&
        text.charCodeAt(next) <= 57
      )
        next++;
      const delimiter = text.charCodeAt(next);
      const after = text.charCodeAt(next + 1);
      if (
        (delimiter === 46 || delimiter === 41) &&
        (next + 1 === end || after === 32 || after === 9)
      )
        marker = next++;
    }
    if (marker < 0) return -1;
    if (++count > MAX_NESTING) return THEMATIC_BREAK.test(text.slice(start, end)) ? -1 : marker;
    at = next;
  }
  return -1;
}

/**
 * `text` with every line's leading container markers past the MAX_NESTING-th escaped (`\>`, `\-`, `1\.`),
 * so the rest of the line is text. A line inside a fenced code block is treated the same way (a code line
 * of 33 or more `>` or `- ` gains a backslash), the price of not parsing first.
 */
export function capNesting(text: string): string {
  if (text.length <= MAX_NESTING) return text;
  let out = "";
  let copied = 0;
  let start = 0;
  while (start < text.length) {
    let end = text.indexOf("\n", start);
    if (end < 0) end = text.length;
    if (end - start > MAX_NESTING) {
      const at = deepMarker(text, start, end);
      if (at >= 0) {
        out += `${text.slice(copied, at)}\\`;
        copied = at;
      }
    }
    start = end + 1;
  }
  return copied === 0 ? text : out + text.slice(copied);
}

/** Whether `root` has a node more than `limit` levels below it (an iterative walk: the tree may be deep). */
export function deeperThan(root: Nodes, limit: number): boolean {
  const stack: [Nodes, number][] = [[root, 0]];
  for (let item = stack.pop(); item; item = stack.pop()) {
    const [node, depth] = item;
    if (depth > limit) return true;
    if ("children" in node) for (const child of node.children) stack.push([child, depth + 1]);
  }
  return false;
}
