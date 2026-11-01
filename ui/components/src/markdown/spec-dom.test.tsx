// @vitest-environment happy-dom
// The spec suites check the HTML of the hast the renderer produces; this checks that the React DOM our
// components make from that hast keeps its text, for every example, and that the differences are the
// intended ones: links keep an href only for https, http, mailto (and fragments, mapped to the answer's
// ids), so relative links are inert; images that are not clauseo-file: or raster data: are placeholders;
// code blocks drop their class for data-lang.
import type { Element, Root as Hast, RootContent } from "hast";
import { act } from "react";
import { createRoot } from "react-dom/client";
import type { Root } from "react-dom/client";
import { afterAll, beforeAll, expect, test } from "vitest";
import spec from "commonmark-spec/spec.txt?raw";
import { createAnswer } from "./answer.ts";
import gfmSpec from "./gfm-spec.json";
import { loadEntities } from "./entities.ts";
import { Markdown } from "./markdown.tsx";
import { examplesOf, withTabs } from "./spec-fixture.ts";

// The full entity table, which the renderer loads lazily (entities.ts), is loaded first.
await loadEntities();

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const EXAMPLES = [...examplesOf(spec), ...gfmSpec.examples.map(withTabs)];

function walk(nodes: readonly RootContent[], visit: (node: RootContent) => void) {
  for (const node of nodes) {
    visit(node);
    if (node.type === "element") walk(node.children, visit);
  }
}

function hastOf(markdown: string): Hast[] {
  return createAnswer("x")
    .update(markdown, false)
    .map((view) => view.hast);
}

const words = (text: string) => text.replace(/\s+/g, "");

let host: HTMLDivElement;
let root: Root;
beforeAll(() => {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterAll(() => {
  act(() => root.unmount());
  host.remove();
});

test("for every spec example the DOM keeps the hast's text, links and images, with only the intended differences", () => {
  const failures: string[] = [];
  EXAMPLES.forEach((example, index) => {
    act(() => root.render(<Markdown key={index} text={example.markdown} streaming={false} />));
    let text = "";
    const links: Element[] = [];
    const images: Element[] = [];
    for (const tree of hastOf(example.markdown)) {
      walk(tree.children, (node) => {
        if (node.type === "text") text += node.value;
        else if (node.type === "element" && node.tagName === "a") links.push(node);
        else if (node.type === "element" && node.tagName === "img") images.push(node);
      });
    }
    const dom = host.firstElementChild?.cloneNode(true) as HTMLElement | undefined;
    if (!dom) throw new Error("nothing rendered");
    const placeholders = [...dom.querySelectorAll('[data-clauseo="image"][data-blocked]')];
    for (const placeholder of placeholders) placeholder.textContent = "";
    const label = `example ${example.example}`;
    if (words(dom.textContent ?? "") !== words(text))
      failures.push(`${label}: text "${dom.textContent}" against "${text}"`);
    const anchors = [...dom.querySelectorAll("a")];
    if (anchors.length !== links.length)
      failures.push(`${label}: ${anchors.length} links against ${links.length}`);
    anchors.forEach((anchor, at) => {
      const href = links[at]?.properties["href"];
      const kept = typeof href === "string" && /^(?:https?:|mailto:)/i.test(href) ? href : null;
      const shownHref = anchor.getAttribute("href");
      // A fragment points at the answer's own id: the sanitiser's prefix, the answer's scope, the fragment.
      const fragment = typeof href === "string" && href.startsWith("#");
      const ok = fragment
        ? shownHref?.startsWith("#user-content-md") && shownHref.endsWith(`-${href.slice(1)}`)
        : shownHref === kept;
      if (anchor.getAttribute("data-clauseo") !== "link")
        failures.push(`${label}: a link not from the link component`);
      if (!ok) failures.push(`${label}: href ${shownHref} against ${String(href)}`);
    });
    const shown = dom.querySelectorAll('[data-clauseo="image"]').length;
    if (shown !== images.length)
      failures.push(`${label}: ${shown} images against ${images.length}`);
    const real = dom.querySelectorAll("img").length;
    if (real + placeholders.length !== images.length)
      failures.push(`${label}: ${real} real images and ${placeholders.length} placeholders`);
  });
  expect(failures).toEqual([]);
}, 30_000);
