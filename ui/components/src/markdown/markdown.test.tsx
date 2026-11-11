// @vitest-environment happy-dom
// The renderer's components at their interface: references validated before anything can open them, links
// that never navigate, fragments that reach the answer's own ids, ids that differ between answers, plain
// code blocks, raw HTML as text and the image placeholder in the page's language.
import { act } from "react";
import { createRoot } from "react-dom/client";
import type { Root } from "react-dom/client";
import { afterAll, afterEach, beforeAll, expect, test, vi } from "vitest";
import { DEFAULT_DISPLAY, showDisplay } from "../lib/display.ts";
import { MarkdownHostContext, PaperLinksContext } from "./host.ts";
import type { MarkdownHost, PaperLinks } from "./host.ts";
import { Markdown } from "./markdown.tsx";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const calls: string[] = [];
const HOST: MarkdownHost = {
  openExternal: (url) => calls.push(`external ${url}`),
  openReference: (kind, id) => calls.push(`${kind} ${id}`),
  openPath: (path) => calls.push(`path ${path}`),
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

test("a link click is cancelled and goes to the host; a file link that names no paper is its text alone", () => {
  show("Read [the act](https://indiacode.nic.in/act) or [this](/local/path).");
  const links = [...host.querySelectorAll("a")];
  expect(links).toHaveLength(1);
  const [external] = links;
  expect(click(external).defaultPrevented).toBe(true);
  expect(click(external, { button: 1 }).defaultPrevented).toBe(true);
  expect(host.textContent).toBe("Read the act or this.");
  expect(host.querySelector('[data-clauseo="link-text"]')?.textContent).toBe("this");
  expect(host.querySelector('[role="link"], [tabindex]')).toBeNull();
  expect(calls).toEqual([
    "external https://indiacode.nic.in/act",
    "external https://indiacode.nic.in/act",
  ]);
});

test("a link click hands the host its visible text, whatever markup is inside", () => {
  const seen: [string, string | undefined][] = [];
  act(() =>
    root.render(
      <MarkdownHostContext
        value={{ ...HOST, openExternal: (url, shown) => seen.push([url, shown]) }}
      >
        <Markdown
          text="See [the **Supreme** Court](https://sci.gov.in/x) and <https://example.com/y>."
          streaming={false}
        />
      </MarkdownHostContext>,
    ),
  );
  const [court, auto] = [...host.querySelectorAll("a")];
  // rendering opens nothing: only a press does
  expect(seen).toEqual([]);
  click(court);
  click(auto);
  expect(seen).toEqual([
    ["https://sci.gov.in/x", "the Supreme Court"],
    ["https://example.com/y", "https://example.com/y"],
  ]);
  // what goes to the host is the text the link renders
  expect([court?.textContent, auto?.textContent]).toEqual(seen.map(([, shown]) => shown));
});

test("an image inside a link counts as U+FFFC in its text, and a hidden character shows the real host", () => {
  const seen: [string, string | undefined][] = [];
  act(() =>
    root.render(
      <MarkdownHostContext
        value={{ ...HOST, openExternal: (url, shown) => seen.push([url, shown]) }}
      >
        <Markdown
          text={
            "[![](data:image/png;base64,iVBORw0KGgo=)](https://evil.io/a) and [bank\uFE0F.com](https://evil.io/b) and [bank\u00B7com](https://evil.io/c) and [the order](https://evil.io/d)"
          }
          streaming={false}
        />
      </MarkdownHostContext>,
    ),
  );
  const links = [...host.querySelectorAll('a[data-clauseo="link"]')];
  for (const link of links) click(link);
  expect(seen.map(([url]) => url)).toEqual([
    "https://evil.io/a",
    "https://evil.io/b",
    "https://evil.io/c",
    "https://evil.io/d",
  ]);
  expect(seen[0]?.[1]).toContain("\uFFFC");
  // the note names the real host where the desktop asks, and only there
  expect(
    links.map((link) => link.querySelector('[data-clauseo="link-host"]')?.textContent ?? null),
  ).toEqual([" (evil.io)", " (evil.io)", " (evil.io)", null]);
});

test("a word that folds into a host, or an image nested in markup, shows the real host (iris's second round)", () => {
  const dots = ["\u1803", "\u1809", "\u0660", "\u06F0", "\uA60E", "\u{1D16D}", "\u{10A50}"];
  const spaces = ["\u200A", "\u2009", "\u00A0", "\u0BCD"];
  const words = [
    ...dots.map((dot) => `ｂａｎｋ${dot}ｃｏｍ`),
    ...spaces.map((space) => `ｂａｎｋ${space}．ｃｏｍ`),
    "𝐛𝐚𝐧𝐤᠃𝐜𝐨𝐦",
    "аррӏе᠃сом",
  ];
  const text = [
    ...words.map((word, k) => `[${word}](https://evil.io/${k})`),
    "[**_![](data:image/png;base64,iVBORw0KGgo=)_**](https://evil.io/nested)",
    "[निर्णय देखें](https://evil.io/plain)",
  ].join(" and ");
  const seen: (string | undefined)[] = [];
  act(() =>
    root.render(
      <MarkdownHostContext value={{ ...HOST, openExternal: (_url, shown) => seen.push(shown) }}>
        <Markdown text={text} streaming={false} />
      </MarkdownHostContext>,
    ),
  );
  const links = [...host.querySelectorAll('a[data-clauseo="link"]')];
  expect(links).toHaveLength(words.length + 2);
  const notes = links.map(
    (link) => link.querySelector('[data-clauseo="link-host"]')?.textContent ?? null,
  );
  expect(notes).toEqual([...words.map(() => " (evil.io)"), " (evil.io)", null]);
  click(links[words.length]);
  expect(seen[0]).toContain("\uFFFC");
});

test("a file link names a paper only through PaperLinksContext: a click or Enter sends that paper's path to openPath, never a navigation", () => {
  const asked: string[] = [];
  const papers: PaperLinks = (href) => {
    asked.push(href);
    return href.endsWith("Reply%20to%20bank.docx") ? "drafts/Reply to bank.docx" : undefined;
  };
  act(() =>
    root.render(
      <MarkdownHostContext value={HOST}>
        <PaperLinksContext value={papers}>
          <Markdown
            text={
              "See [Reply notes.md](Reply%20notes.md), [the draft](/Users/a/M/drafts/Reply%20to%20bank.docx) and [mail](mailto:a@b.in)."
            }
            streaming={false}
          />
        </PaperLinksContext>
      </MarkdownHostContext>,
    ),
  );
  const [draft, mail] = [...host.querySelectorAll("a")];
  // a paper the context doesn't name is plain text; mailto never asks it
  expect(host.querySelectorAll("a")).toHaveLength(2);
  expect(host.textContent).toBe("See Reply notes.md, the draft and mail.");
  expect([...new Set(asked)]).toEqual([
    "Reply%20notes.md",
    "/Users/a/M/drafts/Reply%20to%20bank.docx",
  ]);
  // no href to follow: a link the keyboard reaches, which the host opens
  expect(draft?.hasAttribute("href")).toBe(false);
  expect(draft?.getAttribute("role")).toBe("link");
  expect(draft?.tabIndex).toBe(0);
  expect(draft?.getAttribute("data-clauseo")).toBe("link");
  expect(click(draft).defaultPrevented).toBe(true);
  act(() => {
    draft?.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
  });
  click(mail);
  expect(calls).toEqual([
    "path drafts/Reply to bank.docx",
    "path drafts/Reply to bank.docx",
    "external mailto:a@b.in",
  ]);
  expect(mail?.getAttribute("role")).toBeNull();
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

test("raw HTML never becomes elements, block and inline: markup tags drop, the rest is text", () => {
  show(
    '<div onclick="x()">\nblock\n</div>\n\nInline <b>bold</b> and <img src=x onerror=y>, <Party A> <b>.',
  );
  expect(host.textContent).toContain("block");
  expect(host.textContent).toContain("Inline bold and , <Party A> <b>.");
  expect(host.textContent).not.toContain("onclick");
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
  act(() => root.render(<Markdown text={"Done.\n\nThe **key"} streaming />));
  expect(host.querySelector("strong")?.textContent).toBe("key");
  act(() => root.render(<Markdown text={"Done.\n\nThe **key"} streaming={false} />));
  expect(host.querySelector("strong")).toBeNull();
  expect(host.textContent).toBe("Done.The **key");
});

test("an empty answer renders nothing, streaming or finished, and an answer can grow from empty and back", () => {
  for (const streaming of [true, false]) {
    act(() => root.render(<Markdown text="" streaming={streaming} />));
    expect(host.querySelector('[data-clauseo="markdown"]')?.childElementCount).toBe(0);
  }
  act(() => root.render(<Markdown text="" streaming />));
  act(() => root.render(<Markdown text="a" streaming />));
  expect(host.textContent).toBe("a");
  act(() => root.render(<Markdown text="" streaming />));
  expect(host.textContent).toBe("");
});

test("a host that leaves out streaming gets finished text: no tail repair", () => {
  act(() => root.render(<Markdown text={"Done.\n\nThe **key"} />));
  expect(host.querySelector("strong")).toBeNull();
  expect(host.textContent).toBe("Done.The **key");
});

test("links in the streaming last block are inert until it finishes; finished blocks' links are live", () => {
  const text =
    "See [the act](https://indiacode.nic.in/act).\n\nAnd [the rules](https://indiacode.nic.in/rules), " +
    "[Smith](clauseo-cite:smith-2019) and https://a.example/$$x";
  const render = (streaming: boolean) =>
    act(() =>
      root.render(
        <MarkdownHostContext value={HOST}>
          <Markdown text={text} streaming={streaming} />
        </MarkdownHostContext>,
      ),
    );
  render(true);
  const [finished, tail, autolink] = [...host.querySelectorAll("a")];
  expect(finished?.getAttribute("href")).toBe("https://indiacode.nic.in/act");
  expect(tail?.hasAttribute("href")).toBe(false);
  expect(autolink?.hasAttribute("href")).toBe(false);
  expect(autolink?.textContent).toBe("https://a.example/$$x");
  const citation = host.querySelector('[data-clauseo="citation"]');
  expect(citation?.hasAttribute("role")).toBe(false);
  expect(click(tail).defaultPrevented).toBe(true);
  click(citation);
  click(finished);
  expect(calls).toEqual(["external https://indiacode.nic.in/act"]);
  render(false);
  calls.length = 0;
  click(host.querySelectorAll("a")[1]);
  click(host.querySelector('[data-clauseo="citation"]'));
  expect(calls).toEqual(["external https://indiacode.nic.in/rules", "citation smith-2019"]);
});

test("a half-arrived tag hides nothing while streaming", () => {
  act(() => root.render(<Markdown text={"Note: if a<b then the clause continues"} streaming />));
  expect(host.textContent).toBe("Note: if a<b then the clause continues");
  act(() => root.render(<Markdown text={'Para.\n\n<div class="a"\nmore text'} streaming />));
  expect(host.textContent).toContain('<div class="a"');
  expect(host.textContent).toContain("more text");
});

test.each([
  ["plain text", "[bank.example.com](https://bank.example.com/x)", null],
  ["another host", "[bank.example.com](https://evil.example.net/x)", "evil.example.net"],
  ["bold", "[**bank.example.com**](https://evil.example.net/x)", "evil.example.net"],
  ["code", "[`bank.example.com`](https://evil.example.net/x)", "evil.example.net"],
  [
    "a zero-width space",
    "[bank.example.com\u200b](https://evil.example.net/x)",
    "evil.example.net",
  ],
  [
    "a bidi control",
    "[\u202Abank.example.com\u202C](https://evil.example.net/x)",
    "evil.example.net",
  ],
  ["case and a full stop", "[Bank.Example.Com.](https://evil.example.net/x)", "evil.example.net"],
  ["brackets", "[(bank.example.com)](https://evil.example.net/x)", "evil.example.net"],
  [
    "full-width dots",
    "[bank\uFF0Eexample\uFF0Ecom](https://evil.example.net/x)",
    "evil.example.net",
  ],
  ["the same host, other case", "[BANK.example.com/path](https://bank.example.com/x)", null],
  ["prose", "[the bank's page](https://evil.example.net/x)", null],
  [
    "a host after a word",
    "[Visit bank.example.com](https://evil.example.net/x)",
    "evil.example.net",
  ],
  [
    "a host before an arrow",
    "[bank.example.com ↗](https://evil.example.net/x)",
    "evil.example.net",
  ],
  [
    "a host before words",
    "[bank.example.com (official)](https://evil.example.net/x)",
    "evil.example.net",
  ],
  [
    "a host after an emoji",
    "[🔒 bank.example.com](https://evil.example.net/x)",
    "evil.example.net",
  ],
  [
    "ideographic full stops",
    "[bank\u3002example\u3002com](https://evil.example.net/x)",
    "evil.example.net",
  ],
  ["the real host among words", "[Visit bank.example.com today](https://bank.example.com/x)", null],
  [
    "the real host and another",
    "[bank.example.com or evil.example.net](https://evil.example.net/x)",
    "evil.example.net",
  ],
])("the link-host hint with %s", (_, text, expected) => {
  show(text);
  const hint = host.querySelector('[data-clauseo="link-host"]');
  expect(hint?.textContent ?? null).toBe(expected === null ? null : ` (${expected})`);
});

test.each([
  ["clauseo-file:exhibit-4471", 1],
  ["clauseo-file:../../etc/passwd", 0],
  ["clauseo-file:matters/4471/exhibit.png", 0],
  ["clauseo-file:", 0],
])("an image of the matter's files renders only with an opaque id (%s)", (src, images) => {
  show(`![exhibit](${src})`);
  expect(host.querySelectorAll("img")).toHaveLength(images);
  if (images === 0)
    expect(host.querySelector('[data-clauseo="image"][data-blocked]')).not.toBeNull();
});
