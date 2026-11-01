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
/** The hast elements a heading may sit in (as the mdast flow containers become): no other is walked. */
const CONTAINERS = new Set(["blockquote", "ul", "ol", "li", "dl", "dd"]);

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

/**
 * The headings of `root`, in document order (an iterative walk: the tree may be deep). It goes into
 * CONTAINERS only, never into a paragraph, table or code block: it runs on the streaming block at every
 * update.
 */
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
      if (
        child.type === "element" &&
        (HEADING.test(child.tagName) || CONTAINERS.has(child.tagName))
      )
        stack.push(child);
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
function numberBlockSlugs(slugs: readonly string[], occurrences: Map<string, number>): string[] {
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
 * The sanitised `root` with its headings carrying the ids of `slugs` (one per heading, in order, as
 * headingsIn finds them), each made by `id`: what a block shows when an earlier heading took some of its
 * slugs. Only the headings and the containers above them are copied; the rest is shared with `root`, which
 * stays as it was. Recursive: the trees given have passed answer.ts's depth check.
 */
export function withSlugs(
  root: Root,
  slugs: readonly string[],
  id: (value: string) => string,
): Root {
  let next = 0;
  const copy = <T extends Root | Element>(node: T): T => {
    let children: T["children"] | undefined;
    node.children.forEach((child, index) => {
      if (child.type !== "element") return;
      let made = child;
      if (HEADING.test(child.tagName)) {
        const value = slugs[next++];
        if (value) made = { ...child, properties: { ...child.properties, id: id(value) } };
      } else if (CONTAINERS.has(child.tagName)) made = copy(child);
      if (made === child) return;
      children ??= [...node.children] as T["children"];
      children[index] = made;
    });
    return children ? { ...node, children } : node;
  };
  return copy(root);
}

/** What createSlugNumbering numbers: a view's sanitised hast and its headings' own slugs. */
type Sluggable = { readonly hast: Root; readonly slugs?: readonly string[] };

/**
 * A function giving the views of an answer, in order, every heading's id numbered across the answer: a view
 * whose slugs an earlier heading took becomes a copy with `-1`, `-2`… ids (made by `id`), made again only
 * when those change. It keeps what it made of every view but the last: while those stay the same views (a
 * stream only changes its last block or two), the numbering carries on from there, instead of going over
 * every heading of the answer on every update.
 */
export function createSlugNumbering<V extends Sluggable>(
  id: (value: string) => string,
): (views: readonly V[]) => readonly V[] {
  const renumbered = new WeakMap<V, { key: string; view: V }>();
  const renumber = (view: V, slugs: readonly string[]): V => {
    if (slugs.every((value, at) => value === view.slugs![at])) return view;
    const key = slugs.join("\u0000");
    const known = renumbered.get(view);
    if (known?.key === key) return known.view;
    const copy = { ...view, hast: withSlugs(view.hast, slugs, id) };
    renumbered.set(view, { key, view: copy });
    return copy;
  };
  let before: {
    views: readonly V[];
    count: number;
    out: readonly V[];
    occurrences: ReadonlyMap<string, number>;
  } = { views: [], count: 0, out: [], occurrences: new Map() };
  return (views) => {
    const last = before;
    let from = 0;
    while (from < last.count && views[from] === last.views[from]) from++;
    const reuse = from === last.count;
    if (!reuse) from = 0;
    const occurrences = new Map(reuse ? last.occurrences : undefined);
    const out = reuse ? last.out.slice(0, last.count) : [];
    for (let index = from; index < views.length; index++) {
      if (index === views.length - 1)
        before = { views, count: index, out, occurrences: new Map(occurrences) };
      const view = views[index]!;
      const slugs = view.slugs ?? [];
      out.push(slugs.length === 0 ? view : renumber(view, numberBlockSlugs(slugs, occurrences)));
    }
    return out;
  };
}
