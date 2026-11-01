import type { Nodes as Hast, Root } from "hast";
import type { Root as Mdast } from "mdast";
import { expect, test } from "vitest";
import { createAnswer } from "./answer.ts";
import {
  capNesting,
  deeperThan,
  MAX_BRACKET_SPAN,
  MAX_INDENT,
  MAX_NESTING,
  MAX_OPEN_BRACKETS,
  MAX_RUN,
  MAX_SEQUENCES,
  MAX_UNMATCHED_BRACKETS,
  restore,
  standIn,
} from "./nesting.ts";

// Hostile input (PR #107's security reviews): these overflowed the stack, in marked and in
// mdast-util-to-hast, and unmounted the answer on every render, or froze it in micromark's parse.
const DEEP_LIST = "- ".repeat(1000) + "x";
const DEEP_QUOTE = "> ".repeat(3000) + "x";
/** 500 levels of list built through tabs, which no single line's markers show (round 2's input, 3 KB). */
const TAB_NESTING = Array.from(
  { length: 20 },
  (_, line) => `${"\t".repeat(16 * line)}${"- ".repeat(32)}x\n`,
).join("");
/** The same through spaces. */
const SPACE_NESTING = Array.from(
  { length: 20 },
  (_, line) => `${" ".repeat(60 * line)}${"- ".repeat(30)}x\n`,
).join("");
const LONG_RUNS = "*".repeat(5000) + "a" + "*".repeat(5000);
const hide = (text: string) => text.split("").map(standIn).join("");

const hasStandIn = (text: string) => /[\uFDD1-\uFDDE]/.test(text);

function count(node: Hast, tagName: string): number {
  let total = 0;
  const stack: Hast[] = [node];
  for (let child = stack.pop(); child; child = stack.pop()) {
    if (child.type === "element" && child.tagName === tagName) total++;
    if ("children" in child) for (const grandchild of child.children) stack.push(grandchild);
  }
  return total;
}

function textOf(node: Hast): string {
  if (node.type === "text") return node.value;
  return "children" in node ? node.children.map((child) => textOf(child as Hast)).join("") : "";
}

function depthOf(node: Hast): number {
  return "children" in node && node.children.length > 0
    ? 1 + Math.max(...node.children.map((child) => depthOf(child as Hast)))
    : 0;
}

test("a line's container markers past the 32nd are cut, so the rest of the line is text", () => {
  expect(capNesting(DEEP_LIST)).toBe(
    `${"- ".repeat(MAX_NESTING)}${hide("-")} ${"- ".repeat(1000 - MAX_NESTING - 1)}x`,
  );
  expect(capNesting(`${">".repeat(40)} x`)).toBe(`${">".repeat(32)}${hide(">")}${">".repeat(7)} x`);
  // 22 ordered markers reach column 65: the space after the 22nd is cut, so it is no marker.
  expect(capNesting(`${"1. ".repeat(40)}x`)).toBe(
    `${"1. ".repeat(21)}1.${hide(" ")}${"1. ".repeat(18)}x`,
  );
  expect(capNesting(`a\n${"> - ".repeat(20)}x\nb`)).toBe(
    `a\n${"> - ".repeat(16)}${hide(">")} - ${"> - ".repeat(3)}x\nb`,
  );
});

test("a footnote definition's label counts as a container marker, so the markers after it are cut too", () => {
  // `[^a]: ` + `- `×3,000 got past the limit (its line starts with `[`): a stack overflow, or 2.4 s.
  expect(capNesting(`[^a]: ${"- ".repeat(40)}x`)).toBe(
    `[^a]: ${"- ".repeat(29)}-${hide(" ")}${"- ".repeat(10)}x`,
  );
  expect(capNesting(`> [^a]: [^b]: ${"> ".repeat(40)}x`)).toBe(
    `> [^a]: [^b]: ${"> ".repeat(26)}${hide(">")} ${"> ".repeat(13)}x`,
  );
  // Not a label: the line is left alone.
  expect(capNesting(`[^a b]: ${"- ".repeat(40)}x`)).toBe(`[^a b]: ${"- ".repeat(40)}x`);
  const views = createAnswer("t").update(`x[^a]\n\n[^a]: ${"- ".repeat(3000)}x`, false);
  const note = views.at(-1)?.items?.[0];
  expect(note && count(note.hast, "ul")).toBeLessThanOrEqual(MAX_NESTING);
  expect(note && textOf(note.hast)).toContain("- - - x");
});

test("a line's container prefix stops at column 64, tabs counted to the next multiple of 4", () => {
  expect(MAX_INDENT).toBe(64);
  expect(capNesting(`${"\t".repeat(20)}- x`)).toBe(
    `${"\t".repeat(16)}${hide("\t")}${"\t".repeat(3)}- x`,
  );
  expect(capNesting(`  \t${"\t".repeat(20)}- x`)).toBe(
    `  \t${"\t".repeat(15)}${hide("\t")}${"\t".repeat(4)}- x`,
  );
  expect(capNesting(`${" ".repeat(70)}- x`)).toBe(
    `${" ".repeat(64)}${hide(" ")}${" ".repeat(5)}- x`,
  );
  expect(capNesting(`${"- ".repeat(31)}${" ".repeat(10)}x`)).toBe(
    `${"- ".repeat(31)}  ${hide(" ")}${" ".repeat(7)}x`,
  );
});

test("a run of * and _ keeps 64 characters as delimiters, a paragraph 400 sequences", () => {
  expect(capNesting(`a${"*".repeat(70)}b`)).toBe(`a${"*".repeat(MAX_RUN)}${hide("*").repeat(6)}b`);
  expect(capNesting(`a${"*_".repeat(40)}b`)).toBe(`a${"*_".repeat(32)}${hide("*_").repeat(8)}b`);
  const many = "a* ".repeat(500);
  expect(capNesting(many)).toBe(
    "a* ".repeat(MAX_SEQUENCES) + `a${hide("*")} `.repeat(500 - MAX_SEQUENCES),
  );
  // A blank line starts a new paragraph, with a new budget.
  expect(capNesting(`${"a* ".repeat(MAX_SEQUENCES)}\n\n**b**`)).toBe(
    `${"a* ".repeat(MAX_SEQUENCES)}\n\n**b**`,
  );
});

test("~ counts with * and _: runs of 64, 400 sequences a paragraph; a ~~~ fence line is left alone", () => {
  expect(capNesting(`a${"~".repeat(70)}b`)).toBe(`a${"~".repeat(MAX_RUN)}${hide("~").repeat(6)}b`);
  expect(capNesting("a~ ".repeat(500))).toBe(
    "a~ ".repeat(MAX_SEQUENCES) + `a${hide("~")} `.repeat(500 - MAX_SEQUENCES),
  );
  const fence = `${"~".repeat(100)}\ncode\n${"~".repeat(100)}\n`;
  expect(capNesting(fence)).toBe(fence);
  const late = `${"a~ ".repeat(MAX_SEQUENCES)}\n${fence}`;
  expect(capNesting(late).endsWith(`\n${fence}`)).toBe(true);
  const [, code] = createAnswer("t").update(`text\n\n~~~ts\nlet a = "~~";\n~~~\n`, false);
  expect(code && textOf(code.hast)).toBe('let a = "~~";\n');
});

test("a paragraph keeps 32 `[` open, 64 `]` with none open, and no `[` open over 1,000 characters", () => {
  expect(capNesting(`${"[".repeat(40)}a`)).toBe(
    `${"[".repeat(MAX_OPEN_BRACKETS)}${hide("[").repeat(40 - MAX_OPEN_BRACKETS)}a`,
  );
  expect(capNesting(`a${"]".repeat(70)}`)).toBe(
    `a${"]".repeat(MAX_UNMATCHED_BRACKETS)}${hide("]").repeat(70 - MAX_UNMATCHED_BRACKETS)}`,
  );
  const long = `[${"a".repeat(MAX_BRACKET_SPAN)} b](u)`;
  expect(capNesting(long)).toBe(`${hide("[")}${long.slice(1)}`);
  const fits = `[${"a".repeat(MAX_BRACKET_SPAN - 2)}](u)`;
  expect(capNesting(fits)).toBe(fits);
  // Quote markers and indentation at line starts do not count towards the span.
  const quoted = `> [${"a\n> ".repeat(400)}](u)`;
  expect(capNesting(quoted)).toBe(quoted);
  // A blank line starts a new paragraph, with new budgets; escaped brackets do not count.
  const fresh = `${"[".repeat(MAX_OPEN_BRACKETS)}\n\n[a](u) ${"\\[".repeat(40)}`;
  expect(capNesting(fresh)).toBe(fresh);
});

test.each([
  ["33 code spans holding `[`", "Use `[` ".repeat(33) + "then [a](u)."],
  ["a fence of 40 `[` lines", `\`\`\`\n${"x = a[\n".repeat(40)}\`\`\`\nSee [docs](u).`],
  ["a fence of 100 `[` lines", `\`\`\`\n${"x = a[\n".repeat(100)}\`\`\`\nSee [docs](u).`],
  ["a `~~~` fence of 40 `[` lines", `~~~\n${"x = a[\n".repeat(40)}~~~\nSee [docs](u).`],
])("brackets in code do not use up the paragraph's limit: %s, then a link", (_, text) => {
  const views = createAnswer("t").update(text, false);
  const links = views.reduce((total, view) => total + count(view.hast, "a"), 0);
  expect(links).toBe(1);
  expect(hasStandIn(views.map((view) => JSON.stringify(view.hast)).join(""))).toBe(false);
});

test("a backtick micromark does not read as code cannot hide more than 64 brackets from the limits", () => {
  const hidden = `<a title="\`">${"[".repeat(100)}a\``;
  expect(capNesting(hidden)).toContain(hide("["));
  // An indented fence line is no fence: it does not start the count again.
  const indented = `${"[".repeat(MAX_OPEN_BRACKETS)}\n    \`\`\`\n[a`;
  expect(capNesting(indented)).toContain(hide("["));
});

test("links still work past the bracket limits, and every cut bracket reads as written", () => {
  const text = `${"[".repeat(40)}a${"]".repeat(40)} then [ok](https://b.example/) and [${"x".repeat(1200)}](/far)`;
  const [view] = createAnswer("t").update(text, false);
  const hast = view!.hast;
  expect(count(hast, "a")).toBe(1);
  expect(JSON.stringify(hast)).toContain('"href":"https://b.example/"');
  expect(textOf(hast)).toBe(
    `${"[".repeat(40)}a${"]".repeat(40)} then ok and [${"x".repeat(1200)}](/far)`,
  );
  expect(hasStandIn(JSON.stringify(hast))).toBe(false);
});

test("lines within the limits, thematic breaks and markers after text are left alone", () => {
  const text = [
    `${"> ".repeat(MAX_NESTING)}x`,
    "- ".repeat(50),
    "* ".repeat(50),
    "*".repeat(100),
    `> ${"_".repeat(100)}`,
    `x ${"> ".repeat(50)}`,
    `${"-".repeat(80)}`,
    `${"\t".repeat(16)}x`,
    `${"**a** ".repeat(30)}`,
  ].join("\n");
  expect(capNesting(text)).toBe(text);
});

test("what the limits insert never shows: code, link URLs and text read as written", () => {
  const code = [
    ">".repeat(40),
    " ".repeat(140) + "x",
    "*".repeat(100) + "a",
    "1. ".repeat(40),
  ].join("\n");
  const url = `https://a.example/${"_".repeat(70)}`;
  const text = `\`\`\`\n${code}\n\`\`\`\n\nSee \`${"_".repeat(80)}\` and [the page](${url}).\n`;
  const views = createAnswer("t").update(text, false);
  const [fence, paragraph] = views.map((view) => view.hast);
  expect(fence && textOf(fence)).toBe(`${code}\n`);
  expect(paragraph && textOf(paragraph)).toBe(`See ${"_".repeat(80)} and the page.`);
  const link = paragraph && JSON.stringify(paragraph);
  expect(link).toContain(JSON.stringify(url));
  expect(link && hasStandIn(link)).toBe(false);
  expect(restore(`a${hide("*_ ")}b`)).toBe("a*_ b");
});

test("a stand-in in the input is dropped, so every one the renderer sees is its own", () => {
  expect(capNesting(`a${hide("*")}b*`)).toBe("ab*");
});

test.each([
  ["1,000 nested list items", DEEP_LIST, "ul", "- - - x"],
  ["3,000 nested block quotes", DEEP_QUOTE, "blockquote", "> > > x"],
])("%s render 32 levels deep, the deeper part as text", (_, text, tagName, rest) => {
  for (const streaming of [true, false]) {
    const [view] = createAnswer("t").update(text, streaming);
    expect(view && count(view.hast, tagName)).toBe(MAX_NESTING);
    expect(view && textOf(view.hast)).toContain(rest);
    expect(view && hasStandIn(textOf(view.hast))).toBe(false);
  }
});

test.each([
  ["500 levels of list through tabs", TAB_NESTING, "ul", "- - - x"],
  ["600 levels of list through spaces", SPACE_NESTING, "ul", "- - - x"],
  ["5,000 * a side", LONG_RUNS, "strong", "***a***"],
])(
  "%s render shallowly, the rest as text; the blocks around them render",
  (_, deep, tagName, rest) => {
    const views = createAnswer("t").update(`Intro **bold**.\n\n${deep}\n\nOutro _end_.\n`, false);
    expect(views).toHaveLength(3);
    const [intro, middle, outro] = views.map((view) => view.hast) as [Root, Root, Root];
    expect(count(intro, "strong")).toBe(1);
    expect(count(outro, "em")).toBe(1);
    expect(count(middle, tagName)).toBeGreaterThan(0);
    expect(depthOf(middle)).toBeLessThanOrEqual(80);
    expect(textOf(middle)).toContain(rest);
    expect(hasStandIn(textOf(middle))).toBe(false);
  },
);

test("a tree deeper than the limit is found without recursion", () => {
  let root: Mdast = { type: "root", children: [] };
  for (let level = 0; level < 20_000; level++)
    root = { type: "root", children: [{ type: "blockquote", children: root.children as never }] };
  expect(deeperThan(root, 128)).toBe(true);
  expect(deeperThan({ type: "root", children: [{ type: "text", value: "x" } as never] }, 128)).toBe(
    false,
  );
});
