// @vitest-environment happy-dom
// The renderer's components at their interface: references validated before anything can open them, links
// that never navigate, fragments that reach the answer's own ids, ids that differ between answers, plain
// code blocks, raw HTML as text and the image placeholder in the page's language.
import { act } from "react";
import { createRoot } from "react-dom/client";
import type { Root } from "react-dom/client";
import { afterAll, afterEach, beforeAll, expect, test, vi } from "vitest";
import { DEFAULT_DISPLAY, showDisplay } from "../lib/display.ts";
import { MarkdownHostContext } from "./host.ts";
import type { MarkdownHost } from "./host.ts";
import { Markdown } from "./markdown.tsx";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const calls: string[] = [];
const HOST: MarkdownHost = {
  openExternal: (url) => calls.push(`external ${url}`),
  openReference: (kind, id) => calls.push(`${kind} ${id}`),
};

let host: HTMLDivElement;
let root: Root;
beforeAll(() => {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(() => {
  calls.length = 0;
  act(() => showDisplay(DEFAULT_DISPLAY));
});
afterAll(() => {
  act(() => root.unmount());
  host.remove();
});

function show(...texts: string[]) {
  act(() =>
    root.render(
      <MarkdownHostContext value={HOST}>
        {texts.map((text, index) => (
          <Markdown key={index} text={text} streaming={false} />
        ))}
      </MarkdownHostContext>,
    ),
  );
}

function click(el: Element | null | undefined, init: MouseEventInit = {}): MouseEvent {
  if (!el) throw new Error("nothing to click");
  const event = new MouseEvent("click", { bubbles: true, cancelable: true, ...init });
  act(() => {
    el.dispatchEvent(event);
  });
  return event;
}

test("a valid citation or paper reference opens through the host, by click or by Enter", () => {
  show("See [Smith v Jones](clauseo-cite:smith-2019_3) and [the lease](clauseo-paper:P42).");
  const citation = host.querySelector('[data-clauseo="citation"]');
  const paper = host.querySelector('[data-clauseo="paper"]');
  expect(citation?.getAttribute("role")).toBe("link");
  expect(citation?.textContent).toBe("Smith v Jones");
  click(citation);
  click(paper);
  act(() => {
    citation?.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true }),
    );
  });
  expect(calls).toEqual(["citation smith-2019_3", "paper P42", "citation smith-2019_3"]);
  expect(host.querySelector("a")).toBeNull();
});

test.each([
  "clauseo-cite:javascript:alert(1)",
  "clauseo-paper:../../etc/passwd",
  "clauseo-cite:",
  "clauseo-paper:a b",
  `clauseo-cite:${"x".repeat(129)}`,
])("a reference with a bad id (%s) is plain text, not clickable", (href) => {
  show(`See [the authority](<${href}>).`);
  expect(host.textContent).toBe("See the authority.");
  expect(
    host.querySelector('[role="link"], a, [data-clauseo="citation"], [data-clauseo="paper"]'),
  ).toBeNull();
});

test("a link click is cancelled and goes to the host; a relative link is inert", () => {
  show("Read [the act](https://indiacode.nic.in/act) or [this](/local/path).");
  const [external, relative] = [...host.querySelectorAll("a")];
  expect(click(external).defaultPrevented).toBe(true);
  expect(click(external, { button: 1 }).defaultPrevented).toBe(true);
  expect(relative?.hasAttribute("href")).toBe(false);
  expect(click(relative).defaultPrevented).toBe(true);
  expect(calls).toEqual([
    "external https://indiacode.nic.in/act",
    "external https://indiacode.nic.in/act",
  ]);
});

test("a footnote reference reaches its note's id (one prefix, not two) and scrolls there, not out", () => {
  show("> A claim[^1].\n>\n> [^1]: The note.");
  const reference = host.querySelector("sup a");
  const href = reference?.getAttribute("href") ?? "";
  const target = document.getElementById(href.slice(1));
  expect(href).toMatch(/^#user-content-md[\w-]*-fn-1$/);
  expect(target?.textContent).toContain("The note.");
  const scroll = vi.fn();
  if (target) target.scrollIntoView = scroll;
  click(reference);
  expect(scroll).toHaveBeenCalledOnce();
  expect(calls).toEqual([]);
});

test("two answers on one page never share an id, and every id keeps the sanitiser's prefix", () => {
  show("> One[^1].\n>\n> [^1]: First.", "> Two[^1].\n>\n> [^1]: Second.");
  const ids = [...host.querySelectorAll("[id]")].map((el) => el.id);
  expect(ids.length).toBeGreaterThanOrEqual(4);
  expect(ids.every((id) => id.startsWith("user-content-"))).toBe(true);
  expect(new Set(ids).size).toBe(ids.length);
  const described = [...host.querySelectorAll("[aria-describedby]")].map((el) =>
    el.getAttribute("aria-describedby"),
  );
  expect(described.length).toBe(2);
  expect(described.every((id) => id !== null && document.getElementById(id) !== null)).toBe(true);
});

test("a code block is plain text in a pre, its language as data-lang, no class and no colouring", () => {
  show("```ts\nconst a = <b>1</b>;\n```");
  const pre = host.querySelector("pre");
  expect(pre?.getAttribute("data-lang")).toBe("ts");
  expect(pre?.querySelector("code")?.textContent).toBe("const a = <b>1</b>;");
  expect(pre?.querySelectorAll("[class], [style], span, b")).toHaveLength(0);
});

test("raw HTML is text, block and inline", () => {
  show('<div onclick="x()">\nblock\n</div>\n\nInline <b>bold</b> and <img src=x onerror=y>.');
  expect(host.textContent).toContain('<div onclick="x()">');
  expect(host.textContent).toContain("<b>bold</b>");
  expect(host.querySelectorAll("div[onclick], b, img")).toHaveLength(0);
});

test("a blocked image is a placeholder in the page's language, its alt as the label", () => {
  show("![the exhibit](https://exfil.test/x.png)");
  const placeholder = host.querySelector('[data-clauseo="image"][data-blocked]');
  expect(placeholder?.textContent).toBe("Image blocked");
  expect(placeholder?.getAttribute("aria-label")).toBe("the exhibit");
  act(() => showDisplay({ ...DEFAULT_DISPLAY, lang: "hi" }));
  expect(host.querySelector('[data-clauseo="image"][data-blocked]')?.textContent).toBe(
    "चित्र रोका गया",
  );
});

test("the streaming tail shows repaired emphasis, and the finished text shows the source's", () => {
  act(() => root.render(<Markdown text={"Done.\n\nThe **key"} />));
  expect(host.querySelector("strong")?.textContent).toBe("key");
  act(() => root.render(<Markdown text={"Done.\n\nThe **key"} streaming={false} />));
  expect(host.querySelector("strong")).toBeNull();
  expect(host.textContent).toBe("Done.The **key");
});
