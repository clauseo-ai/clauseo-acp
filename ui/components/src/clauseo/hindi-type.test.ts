// @vitest-environment happy-dom
// Hindi in an English answer sets in the Hindi type: a block the renderer marks lang="hi" (markdown/to-react.ts)
// takes the Devanagari face, Hindi's taller line and no tracking from tokens-type.css, on its own element, in an
// English page; an English block in the Hindi interface keeps the page's; an English run inside Hindi keeps its
// Latin tracking. The sheets are read as written: tokens-type.css whole, base.css's rule for the face, and
// thread.css's answer rules with Tailwind's @apply lines left out (the reply type's line height stands in for them).
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { afterEach, expect, test } from "vitest";

const read = (path: string) => readFileSync(join(import.meta.dirname, path), "utf8");
/**
 * tokens-type.css and thread.css, their :lang() selectors as attribute ones: happy-dom matches no :lang(). Equal
 * here, where every element either carries its lang or takes it from <html>, and custom properties inherit.
 */
const TYPE = read("../styles/tokens-type.css").replace(/:lang\((\w+)\)/g, '[lang="$1"]');
const BASE = read("../styles/base.css");
const THREAD = read("thread.css").replace(/:lang\((\w+)\)/g, '[lang="$1"]');

/** The rules of `css` whose selector line contains `selector`, each with its declarations, from its own text. */
function rules(css: string, selector: string): string {
  const out: string[] = [];
  for (const match of css.matchAll(/([^{}]+)\{([^{}]*)\}/g)) {
    if (match[1]!.includes(selector)) out.push(`${match[1]!.trim()} {${match[2]}}`);
  }
  expect(out.length, selector).toBeGreaterThan(0);
  return out.join("\n");
}

/** What @apply text-reply gives .reply-text, and the body's face and line (base.css), so the tokens resolve. */
const PAGE = `body { font-family: var(--font); line-height: calc(var(--lh-ui-loose) * var(--k)); }
  .reply-text { font-size: var(--t17); line-height: calc(var(--lh-reply) * var(--k)); }
  .tracked, .para { letter-spacing: var(--tr-read); }
  :root { --k: 1; }`;
const ANSWER = `.reply-text [data-clauseo="markdown"]`;

function setUp(lang: "en" | "hi", body: string) {
  const sheets = [
    TYPE,
    rules(BASE, ':where([lang="hi"])'),
    rules(THREAD, ANSWER),
    rules(THREAD, '[lang="hi"] [lang="en"]'),
    PAGE,
  ];
  for (const css of sheets) {
    const style = document.createElement("style");
    style.textContent = css;
    document.head.append(style);
  }
  document.documentElement.lang = lang;
  document.body.innerHTML = `<div class="reply-text"><div data-clauseo="markdown">${body}</div></div>`;
}

const style = (selector: string) => {
  const node = document.querySelector(selector);
  if (node === null) throw new Error(selector);
  return getComputedStyle(node);
};
/** A line height as px: happy-dom resolves the variables but leaves calc() unworked (`calc(29px * 1)`). */
const line = (selector: string) => {
  const value = style(selector).lineHeight;
  const calc = /^calc\((\d+(?:\.\d+)?)px \* (\d+(?:\.\d+)?)\)$/.exec(value);
  return calc ? `${Number(calc[1]) * Number(calc[2])}px` : value;
};
const face = (selector: string) => style(selector).fontFamily.split(",")[0]!.trim();

afterEach(() => {
  document.head.innerHTML = "";
  document.body.innerHTML = "";
  document.documentElement.removeAttribute("lang");
});

test("in the English interface, a Hindi paragraph takes the Devanagari face, the Hindi line, no tracking", () => {
  setUp("en", `<p id="en">Notice.</p><p id="hi" lang="hi" class="tracked">नोटिस।</p>`);
  expect(face("#en")).toBe('"Anek Latin"');
  expect(line("#en")).toBe("27px");
  expect(face("#hi")).toBe('"Anek Devanagari"');
  expect(line("#hi")).toBe("29px");
  expect(style("#hi").letterSpacing).toBe("0px");
});

test("a Hindi heading takes the Hindi line at its own size, and the Devanagari face", () => {
  setUp(
    "en",
    `<h2 id="en">Grounds</h2><h2 id="hi2" lang="hi">आधार</h2><h3 id="hi3" lang="hi">आधार</h3>`,
  );
  expect(line("#en")).toBe("29px");
  expect(line("#hi2")).toBe("31px");
  expect(face("#hi2")).toBe('"Anek Devanagari"');
  expect(line("#hi3")).toBe("29px");
});

test("a Hindi item and quote take the reply's Hindi line, a Hindi cell the table's", () => {
  setUp(
    "en",
    `<ul><li id="li" lang="hi">नोटिस</li></ul><blockquote><p id="q" lang="hi">नोटिस</p></blockquote>
     <table><tbody><tr><td id="td-en">Term</td><td id="td" lang="hi">शर्त</td></tr></tbody></table>`,
  );
  expect(line("#li")).toBe("29px");
  expect(line("#q")).toBe("29px");
  expect(line("#td-en")).toBe("20px");
  expect(line("#td")).toBe("22px");
  expect(face("#td")).toBe('"Anek Devanagari"');
});

test("in the Hindi interface, an English block is left with the page's own Hindi settings", () => {
  setUp("hi", `<p id="en" class="tracked">Notice.</p>`);
  expect(face("#en")).toBe('"Anek Devanagari"');
  expect(line("#en")).toBe("29px");
  expect(style("#en").letterSpacing).toBe("0px");
});

test("an English run marked lang=en inside a Hindi paragraph keeps its Latin tracking", () => {
  setUp("en", `<p class="para" lang="hi" id="hi">नोटिस <span id="run" lang="en">Acme</span></p>`);
  expect(style("#hi").letterSpacing).toBe("0px");
  // 0.002em: happy-dom takes the em at 16 px here
  expect(style("#run").letterSpacing).toBe("0.032px");
});
