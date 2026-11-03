// @vitest-environment happy-dom
// Blocks whose letters are mostly Devanagari take lang="hi" (lib/script.ts), in either interface language, so the
// Hindi type settings follow them (tokens-type.css): a paragraph, a heading, a list item, a table cell, a quoted
// paragraph. Others are left as they are, and the streaming tail takes it as soon as it turns Hindi.
import { act } from "react";
import { createRoot } from "react-dom/client";
import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, expect, test } from "vitest";
import { DEFAULT_DISPLAY, showDisplay } from "../lib/display.ts";
import { createAnswer } from "./answer.ts";
import { appendProbe } from "./append.ts";
import { Markdown } from "./markdown.tsx";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const HI = "किरायेदार को तीस दिन का नोटिस देना होगा।";
const EN = "The tenant must give thirty days' notice.";

function mount(markdown: string, streaming = false) {
  const container = document.createElement("div");
  document.body.append(container);
  const root = createRoot(container);
  act(() => root.render(<Markdown text={markdown} streaming={streaming} />));
  return { container, root };
}

afterEach(() => {
  act(() => showDisplay(DEFAULT_DISPLAY));
  document.body.innerHTML = "";
});

const langs = (container: Element, selector: string) =>
  [...container.querySelectorAll(selector)].map((node) => node.getAttribute("lang"));

test("in the English interface, a Hindi paragraph, heading, item, cell and quote take lang=hi", () => {
  const { container } = mount(
    [
      `## धारा ८ का नोटिस`,
      HI,
      `- ${HI}\n- ${EN}`,
      `| शर्त | Term |\n| - | - |\n| नोटिस अवधि | Notice |`,
      `> ${HI}`,
    ].join("\n\n"),
  );
  expect(document.documentElement.lang).not.toBe("hi");
  expect(langs(container, "h2")).toEqual(["hi"]);
  expect(langs(container, "[data-clauseo='markdown'] > p")).toEqual(["hi"]);
  expect(langs(container, "li")).toEqual(["hi", null]);
  expect(langs(container, "th")).toEqual(["hi", null]);
  expect(langs(container, "td")).toEqual(["hi", null]);
  expect(langs(container, "blockquote p")).toEqual(["hi"]);
});

test("in the Hindi interface, an English block is left as it is", () => {
  act(() => showDisplay({ ...DEFAULT_DISPLAY, lang: "hi" }));
  expect(document.documentElement.lang).toBe("hi");
  const { container } = mount(`## Notice\n\n${EN}\n\n- ${EN}`);
  expect(container.querySelectorAll("[lang]")).toHaveLength(0);
});

test("a mixed block follows the majority of its letters; an item reads its own words, not its sublist's", () => {
  const html = createAnswer("t")
    .update(
      [
        `Acme Traders v. ${HI}`,
        `Under the Rent Control Act, the किरायेदार must give notice.`,
        `- ${HI}\n  - ${EN} ${EN} ${EN}`,
        `- ${EN}\n  - ${HI} ${HI} ${HI}`,
      ].join("\n\n"),
      false,
    )
    .map((view) => renderToStaticMarkup(view.content))
    .join("");
  const doc = new DOMParser().parseFromString(html, "text/html");
  expect(langs(doc.body, "body > p")).toEqual(["hi", null]);
  expect(langs(doc.body, "body > ul > li")).toEqual(["hi", null]);
  expect(langs(doc.body, "li li")).toEqual([null, "hi"]);
});

test("the streaming tail takes lang=hi as its paragraph turns Hindi, on the append fast path", () => {
  let fast = 0;
  appendProbe.check = () => fast++;
  try {
    const answer = createAnswer("t");
    const tail = (text: string) => {
      const views = answer.update(text, true);
      return renderToStaticMarkup(views.at(-1)!.content);
    };
    let text = "Notice under section eight: ";
    expect(tail(text)).toBe(`<p>${text.trimEnd()}</p>`);
    let last = "";
    for (const word of HI.split(" ")) {
      text += `${word} `;
      last = tail(text);
    }
    expect(last).toMatch(/^<p lang="hi">/);
    // and back, as English takes over again
    text += "and the landlord may then end the lease by a written notice of his own";
    expect(tail(text)).toMatch(/^<p>/);
    expect(fast).toBeGreaterThan(0);
  } finally {
    appendProbe.check = undefined;
  }
});

test("the live DOM follows: the streaming paragraph gains lang=hi mid-stream", () => {
  const { container, root } = mount("Notice: ", true);
  expect(container.querySelector("p")?.getAttribute("lang")).toBeNull();
  act(() => root.render(<Markdown text={`Notice: ${HI}`} streaming />));
  expect(container.querySelector("p")?.getAttribute("lang")).toBe("hi");
  act(() => root.render(<Markdown text={`Notice: ${HI}`} streaming={false} />));
  expect(container.querySelector("p")?.getAttribute("lang")).toBe("hi");
});
