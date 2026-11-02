// What a finished block keeps (#233): its React elements, its source and its footnote data, not its mdast or
// hast. What later needs it parsed again (a note, the entity table, a link definition arriving) parses its
// kept input, so the block still renders as a block rendered once would.
import { setFlagsFromString } from "node:v8";
import { runInNewContext } from "node:vm";
import { isValidElement } from "react";
import type { ReactNode } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { expect, test, vi } from "vitest";
import { createAnswer, holdsHast } from "./answer.ts";
import type { BlockView } from "./answer.ts";
import { characterEntitiesLoaded, loadEntities } from "./entities.ts";
import { CASES } from "./hostile-cases.ts";
import { shownHast } from "./view-fixture.ts";

// Every mdast root parsed, and every footnote numbering made, while `tracked.on`: held weakly, so a test can
// tell whether anything still keeps them once the answer has ended.
const tracked = vi.hoisted(() => ({
  on: false,
  roots: [] as WeakRef<object>[],
  numberings: [] as WeakRef<object>[],
}));
vi.mock("./parse.ts", async (original) => {
  const real = await original<typeof import("./parse.ts")>();
  return {
    ...real,
    parse: (source: string) => {
      const root = real.parse(source);
      if (tracked.on) tracked.roots.push(new WeakRef(root));
      return root;
    },
  };
});
vi.mock("./footnotes.ts", async (original) => {
  const real = await original<typeof import("./footnotes.ts")>();
  return {
    ...real,
    numberFootnotes: (...args: Parameters<typeof real.numberFootnotes>) => {
      const numbering = real.numberFootnotes(...args);
      if (tracked.on) tracked.numberings.push(new WeakRef(numbering));
      return numbering;
    },
  };
});

setFlagsFromString("--expose-gc");
const gc = runInNewContext("gc") as () => void;

/** How many of `refs` are still alive after a full collection (a WeakRef holds its target to the job's end). */
async function aliveAfterGc(refs: readonly WeakRef<object>[]): Promise<number> {
  await new Promise((resolve) => setTimeout(resolve, 0));
  gc();
  gc();
  return refs.filter((ref) => ref.deref() !== undefined).length;
}

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
  // The hast a finished view makes again, from its source, is the one its elements were made from (the code
  // block's text and language included).
  for (const view of all) shownHast(view);
  const code = all.find((view) => view.source.startsWith(FENCE))!;
  expect(JSON.stringify(code.content)).toContain('"text":"const a = 1;\\n","lang":"ts"');
  // And the whole answer is the same answer rendered once, element for element.
  const once = viewsOf(createAnswer("f").update(ANSWER, false));
  expect(all.map((view) => view.source)).toEqual(once.map((view) => view.source));
  for (const [index, view] of all.entries()) expect(view.content).toEqual(once[index]!.content);
});

test("every hostile case's views render what their hast says, streaming and finished", () => {
  for (const { id, markdown } of CASES)
    for (const streaming of [true, false])
      for (const view of viewsOf(createAnswer("h").update(markdown, streaming)))
        expect(() => shownHast(view), id).not.toThrow();
  // Every case twice, as hostile-time.test.ts renders them: about 8 s alone.
}, 60_000);

const NOTES =
  Array.from({ length: 12 }, (_, n) => `Claim ${n}[^n${n}] and more prose.\n\n`).join("") +
  Array.from({ length: 12 }, (_, n) => `[^n${n}]: Note ${n}, citing [^n${(n + 1) % 12}].\n\n`).join(
    "",
  );

test("no footnote numbering outlives its update: a note's view keeps only its own numbers", async () => {
  tracked.numberings.length = 0;
  tracked.on = true;
  const answer = createAnswer("k");
  for (const text of prefixes(NOTES)) answer.update(text, true);
  const views = answer.update(NOTES, false);
  tracked.on = false;
  expect(views.at(-1)!.items).toHaveLength(12);
  expect(tracked.numberings.length).toBeGreaterThan(20);
  expect(await aliveAfterGc(tracked.numberings)).toBe(0);
  // Still in use, so the collector could not take the views and their notes.
  expect(answer.update(NOTES, false)).toBe(views);
});

test("after the final render, and its views' hast read again, no block's mdast is kept", async () => {
  const answer = createAnswer("t");
  for (const text of prefixes(ANSWER)) answer.update(text, true);
  tracked.roots.length = 0;
  tracked.on = true;
  const views = answer.update(ANSWER, false);
  for (const view of viewsOf(views)) shownHast(view);
  tracked.on = false;
  // The final render parsed the last block again, and every read of a view's hast parsed its input.
  expect(tracked.roots.length).toBeGreaterThan(9);
  expect(await aliveAfterGc(tracked.roots)).toBe(0);
  expect(answer.update(ANSWER, false)).toBe(views);
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
