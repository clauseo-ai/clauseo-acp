// Heading anchors: GitHub's slugs (github-slugger 2.0.0's algorithm), so a link the agent writes the way
// GitHub would, `[see 4.2](#42-termination)`, finds its heading. A slug is the heading's text in lower
// case, without anything that is not a letter, mark, decimal or letter number, connector punctuation, space
// or hyphen, and with each space made a hyphen; Devanagari and Tamil vowel signs are marks, so Hindi and
// Tamil headings keep them. A repeated slug gets `-1`, `-2`… in document order, across the whole answer.
// The id is scoped to the answer and kept apart from the ids the pipeline makes (`<scope>-h-<slug>`, then
// the sanitiser's `user-content-` prefix), so a heading `# fn-1` never takes a footnote's id; link.tsx
// maps an agent's `#fragment` to it. A heading whose slug is empty gets no id.
import type { Element, ElementContent, Root } from "hast";

const REMOVED = /[^\p{L}\p{M}\p{Nd}\p{Nl}\p{Pc} -]/gu;
const HEADING = /^h[1-6]$/;

/** The id of a heading whose slug is `value`, in the answer `scope` (before the sanitiser's prefix). */
export function headingId(scope: string, value: string): string {
  return `${scope}-h-${value}`;
}

/** GitHub's slug of a heading's text. */
export function slug(text: string): string {
  return text.toLowerCase().replace(REMOVED, "").replace(/ /g, "-");
}

function textOf(nodes: readonly ElementContent[]): string {
  let text = "";
  for (const node of nodes) {
    if (node.type === "text") text += node.value;
    else if (node.type === "element") text += textOf(node.children);
  }
  return text;
}

/** The headings of `root`, in document order (an iterative walk: the tree may be deep). */
function headingsIn(root: Root): Element[] {
  const found: Element[] = [];
  const stack: (Root | Element)[] = [root];
  for (let node = stack.pop(); node; node = stack.pop()) {
    if (node.type === "element" && HEADING.test(node.tagName)) {
      found.push(node);
      continue;
    }
    for (let index = node.children.length - 1; index >= 0; index--) {
      const child = node.children[index]!;
      if (child.type === "element") stack.push(child);
    }
  }
  return found;
}

/**
 * Gives each heading of `root` the id of its own slug, scoped by `scope` (before the sanitiser adds its
 * prefix), in place. Returns the slugs, in order, "" for a heading that got none.
 */
export function anchorHeadings(root: Root, scope: string): string[] {
  return headingsIn(root).map((heading) => {
    const value = slug(textOf(heading.children));
    if (value !== "") heading.properties["id"] = headingId(scope, value);
    return value;
  });
}

/**
 * The final slug of every heading of every block, github-slugger's way: a slug seen before gets the first
 * free `-n`. `perBlock` holds each block's own slugs, in order.
 */
export function numberSlugs(perBlock: readonly (readonly string[])[]): string[][] {
  const occurrences = new Map<string, number>();
  return perBlock.map((slugs) => numberBlockSlugs(slugs, occurrences));
}

/**
 * One block's final slugs, after the blocks whose slugs `occurrences` has seen (updated in place): what
 * numberSlugs does for each block, so the answer can carry on from the blocks it numbered last update.
 */
export function numberBlockSlugs(
  slugs: readonly string[],
  occurrences: Map<string, number>,
): string[] {
  return slugs.map((value) => {
    if (value === "") return value;
    let result = value;
    while (occurrences.has(result)) {
      const count = occurrences.get(value)! + 1;
      occurrences.set(value, count);
      result = `${value}-${count}`;
    }
    occurrences.set(result, 0);
    return result;
  });
}

/**
 * A copy of the sanitised `root` whose headings carry the ids of `slugs` (one per heading, in order), each
 * made by `id`: what a block shows when an earlier heading took some of its slugs.
 */
export function withSlugs(
  root: Root,
  slugs: readonly string[],
  id: (value: string) => string,
): Root {
  const copy = structuredClone(root);
  headingsIn(copy).forEach((heading, index) => {
    const value = slugs[index];
    if (value) heading.properties["id"] = id(value);
  });
  return copy;
}
