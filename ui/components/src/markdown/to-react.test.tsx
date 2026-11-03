import type { Element, ElementContent, Properties, Root } from "hast";
import { isValidElement } from "react";
import type { ReactElement, ReactNode } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { expect, test } from "vitest";
import { createAnswer } from "./answer.ts";
import { SCHEMA } from "./schema.ts";
import { MAPPED, toReact } from "./to-react.ts";

// The hast-to-React mapper renders exactly what the sanitise schema allows. Anything else that reaches it
// (as if the sanitiser had let it through) is dropped, never passed on to React.

const element = (
  tagName: string,
  properties: Properties = {},
  children: ElementContent[] = [],
): Element => ({ type: "element", tagName, properties, children });
const text = (value: string): ElementContent => ({ type: "text", value });
const root = (...children: Root["children"]): Root => ({ type: "root", children });
const html = (tree: Root) => renderToStaticMarkup(toReact(tree));
const render = (markdown: string) =>
  createAnswer("t")
    .update(markdown, false)
    .map((view) => renderToStaticMarkup(toReact(view.hast)))
    .join("");

test("every tag and attribute the schema allows has a mapping, and nothing more", () => {
  expect([...MAPPED.tags].sort()).toEqual([...(SCHEMA.tagNames ?? [])].sort());
  for (const [tag, allowed] of Object.entries(SCHEMA.attributes ?? {})) {
    const names = allowed.map((entry) => (Array.isArray(entry) ? entry[0] : entry));
    expect([...MAPPED.attributes(tag)].sort(), tag).toEqual([...names].sort());
  }
});

test("unknown tags are dropped with everything inside them", () => {
  const tree = root(
    element("p", {}, [text("kept")]),
    ...["script", "style", "iframe", "div", "svg", "object", "form", "math", "span"].map((tag) =>
      element(tag, { src: "https://evil.example" }, [text(`inside ${tag}`)]),
    ),
    element("p", {}, [element("blink", {}, [text("blink")]), text("after")]),
  );
  expect(html(tree)).toBe("<p>kept</p><p>after</p>");
});

test("unknown properties, event handlers and inner HTML never reach React", () => {
  const hostile: Properties = {
    onClick: "alert(1)",
    onError: "alert(1)",
    style: "color:red",
    dangerouslySetInnerHTML: "<img src=x onerror=alert(1)>",
    srcDoc: "<script>alert(1)</script>",
    formAction: "javascript:alert(1)",
    id: "clobber",
    className: ["x"],
    title: "t",
    children: "<b>injected</b>",
  };
  const tree = root(
    element("p", hostile, [text("para")]),
    element("blockquote", hostile, [text("quote")]),
    element("h1", hostile, [text("head")]),
    element("td", { ...hostile, align: "left;background:url(x)" }, [text("cell")]),
    element("code", { ...hostile, className: ["language-ts"] }, [text("c")]),
  );
  expect(html(tree)).toBe(
    // A heading's id is its anchor (the sanitiser has prefixed it by now); nothing else of it is kept.
    '<p>para</p><blockquote>quote</blockquote><h1 id="clobber">head</h1><td>cell</td><code class="language-ts" data-clauseo="inline-code">c</code>',
  );
  const [first] = elements(toReact(tree));
  expect(Object.keys(first?.props ?? {})).toEqual(["children"]);
});

test("properties named like Object.prototype's own are not mapped", () => {
  const tree = root(
    element("ol", { constructor: "x", toString: "y", hasOwnProperty: "w" } as never),
    element("code", { constructor: "x", valueOf: "v", className: ["language-a"] } as never),
  );
  expect(html(tree)).toBe('<ol></ol><code class="language-a" data-clauseo="inline-code"></code>');
  // React drops an invalid prop name from the markup, so the props themselves are checked.
  const [, ol, code] = elements(toReact(tree));
  expect(Object.keys(ol?.props ?? {})).toEqual([]);
  expect(Object.keys(code?.props ?? {})).toEqual(["className", "data-clauseo"]);
});

test("values that are not strings, numbers, booleans or lists of them are dropped", () => {
  const tree = root(
    element("ol", { start: { valueOf: () => 3 } as never, className: ["contains-task-list"] }),
  );
  expect(html(tree)).toBe('<ol class="contains-task-list"></ol>');
});

test("comments, doctypes and raw nodes render nothing", () => {
  const tree = {
    type: "root",
    children: [
      { type: "comment", value: "<script>alert(1)</script>" },
      { type: "doctype" },
      { type: "raw", value: "<img src=x onerror=alert(1)>" },
      element("p", {}, [{ type: "raw", value: "<b>raw</b>" } as never, text("ok")]),
    ],
  } as unknown as Root;
  expect(html(tree)).toBe("<p>ok</p>");
});

test("text is escaped by React", () => {
  expect(html(root(element("p", {}, [text('<img src=x onerror="alert(1)"> & </p>')])))).toBe(
    "<p>&lt;img src=x onerror=&quot;alert(1)&quot;&gt; &amp; &lt;/p&gt;</p>",
  );
});

test("property names: class, table-cell alignment as a style, task-list checkboxes, list start", () => {
  expect(render("- [x] done\n- [ ] open")).toBe(
    '<ul class="contains-task-list">\n<li class="task-list-item"><input type="checkbox" disabled="" checked=""/> done</li>\n' +
      '<li class="task-list-item"><input type="checkbox" disabled=""/> open</li>\n</ul>',
  );
  expect(render("| a | b | c |\n|:--|:-:|--:|\n| 1 | 2 | 3 |")).toBe(
    '<div data-clauseo="table" class="max-w-full overflow-x-auto"><table class="break-words">' +
      '<thead><tr><th style="text-align:left">a</th><th style="text-align:center">b</th>' +
      '<th style="text-align:right">c</th></tr></thead><tbody><tr><td style="text-align:left">1</td>' +
      '<td style="text-align:center">2</td><td style="text-align:right">3</td></tr></tbody></table></div>',
  );
  expect(render("3. three\n4. four")).toBe('<ol start="3">\n<li>three</li>\n<li>four</li>\n</ol>');
  expect(render("```ts\nlet a\n```")).toBe(
    '<pre data-clauseo="code" data-lang="ts"><code>let a</code></pre>',
  );
});

test("void elements get no children, so React never throws on them", () => {
  expect(html(root(element("p", {}, [text("a"), element("br", {}, [text("x")]), text("b")])))).toBe(
    "<p>a<br/>b</p>",
  );
  expect(html(root(element("hr", {}, [text("x")])))).toBe("<hr/>");
});

/** The React elements among `node` and its children, depth first. */
function elements(node: ReactNode): ReactElement<{ children?: ReactNode }>[] {
  if (Array.isArray(node)) return node.flatMap(elements);
  if (!isValidElement<{ children?: ReactNode }>(node)) return [];
  return [node, ...elements(node.props.children)];
}

test("keys are the tag and its count among its siblings, the same on every render", () => {
  const [view] = createAnswer("t").update("a *b* *c* **d** `e`", false);
  const keys = (tree: Root) => elements(toReact(tree)).map((node) => node.key);
  expect(keys(view!.hast)).toEqual([null, "p-0", "em-0", "em-1", "strong-0", "code-0"]);
  expect(keys(structuredClone(view!.hast))).toEqual(keys(view!.hast));
});

test("inline code carries data-clauseo=\"inline-code\", which the answer's styles key on; a code block's code does not", () => {
  // the WebView2 shaping run found inline code unstyled: thread.css styles code[data-clauseo="inline-code"], and
  // nothing set it
  const out = render("See `धारा १३८` here.\n\n```text\nक्ष त्र\n```");
  expect(out).toContain('<code data-clauseo="inline-code">धारा १३८</code>');
  expect(out.match(/data-clauseo="inline-code"/g)?.length).toBe(1);
});
