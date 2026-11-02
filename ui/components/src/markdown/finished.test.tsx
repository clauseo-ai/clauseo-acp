// What a finished block keeps (#233): its React elements, its source and its footnote data, not its mdast or
// hast. What later needs it parsed again (a note, the entity table, a link definition arriving) parses its
// kept input, so the block still renders as a block rendered once would.
import { isValidElement } from "react";
import type { ReactNode } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { expect, test } from "vitest";
import { createAnswer, holdsHast } from "./answer.ts";
import type { BlockView } from "./answer.ts";
import { characterEntitiesLoaded, loadEntities } from "./entities.ts";
import { toReact } from "./to-react.ts";

const FENCE = "```";

/** Every 16-character prefix of `text`, then the whole. */
function* prefixes(text: string): Generator<string> {
  for (let end = 16; end < text.length; end += 16) yield text.slice(0, end);
  yield text;
}

/** The block views and the note views of an update. */
const viewsOf = (views: readonly BlockView[]) => views.flatMap((view) => view.items ?? [view]);

/**
 * The path of the first mdast or hast node reachable from `value` through plain data (a view's own
 * fields, its React elements and their props), or undefined: a node is an object with a node's `type`
 * and its `children`, `tagName`, `value` or `position`.
 */
function treeIn(value: unknown, path = "view", seen = new Set<unknown>()): string | undefined {
  if (typeof value !== "object" || value === null || seen.has(value)) return undefined;
  seen.add(value);
  if (isValidElement(value)) return treeIn(value.props, `${path}.props`, seen);
  const record = value as Record<string, unknown>;
  if (
    typeof record["type"] === "string" &&
    ("children" in record || "tagName" in record || "position" in record || "value" in record)
  )
    return path;
  if (Array.isArray(value)) {
    for (const [index, item] of value.entries()) {
      const found = treeIn(item, `${path}[${index}]`, seen);
      if (found) return found;
    }
    return undefined;
  }
  for (const key of Object.keys(record)) {
    const descriptor = Object.getOwnPropertyDescriptor(record, key)!;
    // A view's `hast` is made again when read: what it holds is checked with holdsHast.
    if (descriptor.get) continue;
    const found = treeIn(descriptor.value, `${path}.${key}`, seen);
    if (found) return found;
  }
  return undefined;
}

const html = (content: ReactNode) => renderToStaticMarkup(<>{content}</>);

const ANSWER = [
  "# Review",
  "A paragraph with *emphasis*, a [link](https://example.com/a), a call[^a] and `code`.",
  "## Review",
  "- an item with a [reference][ref]\n- and a second[^b]",
  `${FENCE}ts\nconst a = 1;\n${FENCE}`,
  "| a | b |\n| - | - |\n| 1 | 2 |",
  "[^a]: The first note, with a [link](https://example.com/n).",
  "[ref]: https://example.com/ref",
  "[^b]: The second note.",
  "Last words &amp; more.",
].join("\n\n");

test("no view holds its mdast or hast once its update has ended, and its elements match its hast", () => {
  const answer = createAnswer("f");
  let views: readonly BlockView[] = [];
  for (const text of prefixes(ANSWER)) {
    views = answer.update(text, true);
    for (const view of [...views, ...viewsOf(views)]) {
      expect(holdsHast(view), view.source).toBe(false);
      expect(treeIn(view), view.source).toBeUndefined();
    }
  }
  views = answer.update(ANSWER, false);
  const all = viewsOf(views);
  expect(all.length).toBeGreaterThan(9);
  for (const view of [...views, ...all]) {
    expect(holdsHast(view), view.source).toBe(false);
    expect(treeIn(view), view.source).toBeUndefined();
  }
  // The hast a finished view makes again, from its source, is the one its elements were made from.
  for (const view of all.filter((one) => !one.source.startsWith(FENCE)))
    expect(html(view.content)).toBe(html(toReact(view.hast, view.continues)));
  // And the whole answer reads as the same answer rendered once.
  const once = viewsOf(createAnswer("f").update(ANSWER, false));
  expect(all.map((view) => view.source)).toEqual(once.map((view) => view.source));
  for (const [index, view] of all.entries())
    if (!view.source.startsWith(FENCE)) expect(html(view.content)).toBe(html(once[index]!.content));
});

test("a note arriving after its reference's block finished links the reference", () => {
  const text =
    "A claim[^n] to support.\n\nA second paragraph.\n\nA third one.\n\n[^n]: The source.\n";
  const answer = createAnswer("n");
  let first: BlockView | undefined;
  for (const prefix of prefixes(text.slice(0, text.indexOf("[^n]:")))) {
    first = answer.update(prefix, true)[0];
  }
  expect(holdsHast(first!)).toBe(false);
  expect(html(first!.content)).toContain("data-footnote-pending");
  let views: readonly BlockView[] = [];
  for (const prefix of prefixes(text)) views = answer.update(prefix, true);
  views = answer.update(text, false);
  expect(views[0]).not.toBe(first);
  expect(html(views[0]!.content)).toMatch(/<a href="#[^"]*-fn-n"/);
  expect(html(views[0]!.content)).not.toContain("data-footnote-pending");
  expect(html(views.at(-1)!.items![0]!.content)).toContain("The source.");
});

test("a link definition arriving after the block that uses it finished resolves it", () => {
  const text =
    "See [the Act][act] here.\n\nMore prose follows.\n\n[act]: https://example.com/act\n\nEnd.\n";
  const answer = createAnswer("d");
  let first: BlockView | undefined;
  for (const prefix of prefixes(text.slice(0, text.indexOf("[act]:")))) {
    first = answer.update(prefix, true)[0];
  }
  expect(html(first!.content)).not.toContain("href");
  let views: readonly BlockView[] = [];
  for (const prefix of prefixes(text)) views = answer.update(prefix, true);
  // The definition counts once its block is no longer the streaming last one.
  expect(html(views[0]!.content)).toContain("https://example.com/act");
  views = answer.update(text, false);
  expect(html(views[0]!.content)).toContain('href="https://example.com/act"');
  expect(holdsHast(views[0]!)).toBe(false);
});

test("the entity table arriving after a block that missed a name finished renders the name", async () => {
  expect(characterEntitiesLoaded()).toBe(false);
  const text = "A plain block.\n\nA rare &ngE; name.\n\nMore prose.\n\nAnd more prose to stream.\n";
  const answer = createAnswer("e");
  let views: readonly BlockView[] = [];
  const cut = text.indexOf("And");
  for (const prefix of prefixes(text.slice(0, cut))) views = answer.update(prefix, true);
  const [plain, rare] = views;
  expect(holdsHast(rare!)).toBe(false);
  expect(html(rare!.content)).toContain("&amp;ngE;");

  await loadEntities();
  views = answer.update(text.slice(0, cut + 3), true);
  expect(views[1]).not.toBe(rare);
  expect(views[0]).toBe(plain);
  expect(html(views[1]!.content)).toContain("≧̸");
  views = answer.update(text, false);
  expect(html(views[1]!.content)).toContain("≧̸");
});
