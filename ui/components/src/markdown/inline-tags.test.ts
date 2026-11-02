// The approved inline tags (inline-tags.ts) through the answer model: what renders as our element and what
// stays text, finished and in the streaming last block, where nothing half-arrived may hide what follows.
import { renderToStaticMarkup } from "react-dom/server";
import { expect, test } from "vitest";
import { createAnswer } from "./answer.ts";
import { ALLOWED_TAGS, MAX_TAG_NESTING } from "./inline-tags.ts";
import { htmlOf } from "./spec-fixture.ts";
import { shownHast } from "./view-fixture.ts";
import { toReact } from "./to-react.ts";

const html = (text: string, streaming = false) =>
  htmlOf(createAnswer("t").update(text, streaming)).replace(/\n/g, "");
/** The streaming last block's HTML. */
const tail = (text: string) =>
  htmlOf(createAnswer("t").update(text, true).slice(-1)).replace(/\n/g, "");
const count = (markup: string, tag: string) => markup.split(`<${tag}>`).length - 1;

test("the allowlist is #137's starting list, exactly", () => {
  expect([...ALLOWED_TAGS].sort()).toEqual(
    ["br", "del", "ins", "kbd", "mark", "s", "small", "sub", "sup", "u"].sort(),
  );
});

test("each allowlisted tag renders as its own element, and <br> in each of its forms as a line break", () => {
  expect(
    html(
      "a <sup>1</sup> <sub>2</sub> <u>3</u> <ins>4</ins> <del>5</del> <s>6</s> <mark>7</mark> <small>8</small> <kbd>9</kbd> b<br>c<br/>d<br />e",
    ),
  ).toBe(
    "<p>a <sup>1</sup> <sub>2</sub> <u>3</u> <ins>4</ins> <del>5</del> <s>6</s> <mark>7</mark> <small>8</small> <kbd>9</kbd> b<br>c<br>d<br>e</p>",
  );
});

test("every attribute is dropped, whatever it is; tag names match ASCII case-insensitively", () => {
  expect(
    html(
      `<sup class="x" style="color:red" onclick="alert(1)" id="y" data-footnote-pending="1">1</sup> <SUP>2</Sup> <Br Class="x">`,
    ),
  ).toBe("<p><sup>1</sup> <sup>2</sup> <br></p>");
  expect(html(`<mark\nstyle="x">a</mark>`)).toBe("<p><mark>a</mark></p>");
});

test("an HTML-named tag not on the list drops when it is clearly markup, its content kept; else it is text", () => {
  for (const tag of [
    "script",
    "style",
    "iframe",
    "img",
    "a",
    "object",
    "svg",
    "math",
    "form",
    "input",
    "button",
    "select",
    "textarea",
    "div",
    "p",
    "table",
    "blockquote",
    "h1",
    "ul",
    "pre",
    "b",
    "span",
  ]) {
    // Paired in one parent, self-closed, or with a valued attribute: markup, dropped.
    expect(html(`x <${tag}>y</${tag}> z`)).toBe("<p>x y z</p>");
    expect(html(`x <${tag}/> <${tag} data-a="1"> z`)).toBe("<p>x   z</p>");
    // Alone, with no valued attribute (a placeholder may share the name), or a stray closing tag: text.
    for (const lone of [`<${tag}>`, `<${tag} of y>`, `</${tag}>`])
      expect(html(`x ${lone} z`)).toBe(`<p>x ${lone.replace("<", "&#x3C;")} z</p>`);
  }
  expect(html("Send it to <Address>, <Address of Tenant>; <Title> of <Section> 4.")).toBe(
    "<p>Send it to &#x3C;Address>, &#x3C;Address of Tenant>; &#x3C;Title> of &#x3C;Section> 4.</p>",
  );
  expect(html("a <section><Party A></section> b")).toBe("<p>a &#x3C;Party A> b</p>");
  // A never-markup element's content is one plain text: no markup in it, tags as written.
  expect(html("a <script>alert(<b>1</b>) *x*</script> b <SVG><i>c</i></svg>")).toBe(
    "<p>a alert(&#x3C;b>1&#x3C;/b>) x b &#x3C;i>c&#x3C;/i></p>",
  );
  // Line-start kind 1 openers (made text by nesting.ts) follow the same rule.
  expect(html("<script>alert(1)</script>\n\n<pre>\nkept\n</pre>\n\n<style lang=x>")).toBe(
    "<p>alert(1)</p><p>kept</p><p></p>",
  );
  // An HTML block: its markup tags drop, the rest stays its text.
  expect(html('<div align="right">\nA: <b>x</b> <Party A>\n</div>')).toBe("A: x &#x3C;Party A>");
});

test("tags not on the list, entities, lookalikes and broken tags show as text, as written", () => {
  for (const tag of ["party", "insert", "client", "font-face", "x-tag"])
    expect(html(`x <${tag}>y</${tag}>`)).toBe(`<p>x &#x3C;${tag}>y&#x3C;/${tag}></p>`);
  expect(html("Made on <insert date> between <Party A> and <Party B>.")).toBe(
    "<p>Made on &#x3C;insert date> between &#x3C;Party A> and &#x3C;Party B>.</p>",
  );
  expect(html("&lt;sup&gt;1&lt;/sup&gt; &#60;sup&#62;2&#60;/sup&#62; &#x3C;sup>3&#x3C;/sup>")).toBe(
    "<p>&#x3C;sup>1&#x3C;/sup> &#x3C;sup>2&#x3C;/sup> &#x3C;sup>3&#x3C;/sup></p>",
  );
  expect(html("< sup>a</ sup> <\u0455up>b</\u0455up> <sup/onload=x>c</sup>")).toBe(
    "<p>&#x3C; sup>a&#x3C;/ sup> &#x3C;\u0455up>b&#x3C;/\u0455up> &#x3C;sup/onload=x>c&#x3C;/sup></p>",
  );
});

test("finished, an unpaired tag, a stray closer, a self-closed tag and </br> show as text", () => {
  expect(html("Fee <sup>2 and the rest")).toBe("<p>Fee &#x3C;sup>2 and the rest</p>");
  expect(html("a </sup> b </br> c <sup/> d")).toBe(
    "<p>a &#x3C;/sup> b &#x3C;/br> c &#x3C;sup/> d</p>",
  );
});

test("misnested tags: the inner unpaired one shows as text; tags never pair across parents", () => {
  expect(html("<sup><sub>a</sup></sub>")).toBe("<p><sup>&#x3C;sub>a</sup>&#x3C;/sub></p>");
  expect(html("*a <sup>b* c</sup>")).toBe("<p><em>a &#x3C;sup>b</em> c&#x3C;/sup></p>");
  expect(html("<u>a\n\nb</u>")).toBe("<p>&#x3C;u>a</p><p>b&#x3C;/u></p>");
  expect(html("*a <sup>b</sup> c*")).toBe("<p><em>a <sup>b</sup> c</em></p>");
});

test("tags never pair across a block sibling or a paragraph break a list item or definition unwraps", () => {
  const heading = html("- <sup>a\n  # h\n  b</sup>");
  const fence = html("Term\n: <mark>a\n  ```\n  code\n  ```\n  b</mark>");
  const paragraphs = html("Term\n: <sup>a\n\n  b</sup>");
  expect(heading).toContain("&#x3C;sup>a");
  expect(heading).toContain("b&#x3C;/sup>");
  expect(fence).toContain("&#x3C;mark>a");
  expect(fence).toContain("b&#x3C;/mark>");
  expect(paragraphs).toContain("&#x3C;sup>a");
  expect(paragraphs).toContain("b&#x3C;/sup>");
  for (const out of [heading, fence, paragraphs]) expect(out).not.toMatch(/<(sup|mark)>/);
  // Within one unwrapped paragraph, and in each one on its own, tags still pair.
  expect(html("- <sup>a</sup>\n  # h\n  <sub>b</sub>")).toMatch(/<sup>a<\/sup>[^]*<sub>b<\/sub>/);
  expect(html("Term\n: <sup>a\n  b</sup>")).toContain("<sup>ab</sup>");
  // A second paragraph that starts with markup is a barrier too.
  for (const start of ["*b*", "**b**", "[b](https://ok.example/)", "`b`", "^x^", "==x=="]) {
    const out = html(`Term\n: <sup>a\n\n  ${start}</sup>`);
    expect(out, start).toContain("&#x3C;sup>a");
    expect(out, start).toContain("&#x3C;/sup>");
    expect(out, start).not.toMatch(/<sup>a/);
  }
});

test("tags in code show as code; in link text, table cells, headings and notes they render", () => {
  expect(html("`<sup>1</sup>`")).toBe("<p><code>&#x3C;sup>1&#x3C;/sup></code></p>");
  expect(html("[clause 4<sup>A</sup>](https://ok.example/)")).toBe(
    '<p><a href="https://ok.example/">clause 4<sup>A</sup></a></p>',
  );
  expect(html("| a |\n| - |\n| 4<sup>A</sup><br>b |")).toContain("<td>4<sup>A</sup><br>b</td>");
  expect(html("## Clause 4<sup>A</sup>")).toBe(
    '<h2 id="user-content-t-h-clause-4a">Clause 4<sup>A</sup></h2>',
  );
  expect(html("A[^n].\n\n[^n]: See <sup>2</sup>.")).toContain("See <sup>2</sup>.");
});

test("an HTML block of nothing but <br> forms is line breaks; any other HTML block stays text", () => {
  expect(html("a\n\n<br>\n<br/>\n\nb")).toBe("<p>a</p><br><br><p>b</p>");
  expect(html("a\n\n<sup>\nb\n</sup>")).toBe("<p>a</p>&#x3C;sup>b&#x3C;/sup>");
  expect(html('a\n\n<br class="x">')).toBe('<p>a</p>&#x3C;br class="x">');
});

test(`past ${MAX_TAG_NESTING} allowlisted elements inside each other, the deeper tags are text`, () => {
  const markup = html("<sup>".repeat(40) + "x" + "</sup>".repeat(40));
  expect(count(markup, "sup")).toBe(MAX_TAG_NESTING);
  expect(markup).toContain("&#x3C;sup>x&#x3C;/sup>");
});

test("streaming: an unpaired opening tag at the end is hidden and what follows shows unstyled", () => {
  expect(tail("Fee <sup>2 and the rest")).toBe("<p>Fee 2 and the rest</p>");
  expect(tail("Fee <sup>2</sup> due")).toBe("<p>Fee <sup>2</sup> due</p>");
  expect(tail("Fee <sup>2</su")).toBe("<p>Fee 2</p>");
  expect(tail("a <su")).toBe("<p>a</p>");
  expect(tail("a </ma")).toBe("<p>a</p>");
  expect(tail("a <")).toBe("<p>a</p>");
  // Once more than a tag name has arrived, it shows as written until it is a tag.
  expect(tail("a <sup cla")).toBe("<p>a &#x3C;sup cla</p>");
  // Once a quoted attribute value has begun, the tag is hidden as its name was, until its `>` arrives.
  expect(tail('a <sup class="x')).toBe("<p>a</p>");
  expect(tail("a <SUP\nstyle = 'color: red")).toBe("<p>a</p>");
  expect(tail('Fee <sup class="x">2')).toBe("<p>Fee 2</p>");
  // An HTML element's tag with a quoted value begun is markup, dropped once whole: hidden till then. A
  // placeholder with no value shows as written.
  expect(tail('a <div class="x')).toBe("<p>a</p>");
  expect(tail('Cap: <div align="right">A: 1')).toBe("<p>Cap: A: 1</p>");
  expect(tail("Serve <Address of Ten")).toBe("<p>Serve &#x3C;Address of Ten</p>");
  expect(tail('a <party class="x')).toBe('<p>a &#x3C;party class="x</p>');
  expect(tail(`a <sup title="${"x".repeat(130)}`)).toContain("&#x3C;sup title=");
  expect(tail("if x<s then the clause")).toBe("<p>if x&#x3C;s then the clause</p>");
  expect(tail("a <b")).toBe("<p>a</p>");
  expect(tail("a <bo")).toBe("<p>a &#x3C;bo</p>");
  // Where no closing tag can arrive any more, it shows as text, as it will finished.
  expect(tail("*a <sup>b* c")).toBe("<p><em>a &#x3C;sup>b</em> c</p>");
  expect(tail("<sup><sub>a</sup> b")).toBe("<p><sup>&#x3C;sub>a</sup> b</p>");
  expect(tail("- <sup>a\n- b")).toBe("<ul><li>&#x3C;sup>a</li><li>b</li></ul>");
  // A stray closer shows.
  expect(tail("a </sup> b")).toBe("<p>a &#x3C;/sup> b</p>");
});

test("streamed a character at a time, a tag never hides or restyles the text after it", () => {
  const text =
    "Fee <sup>2</sup> due <mark>now</mark>, <kbd>Ctrl</kbd> and <u>x <s>y</u> z</s> end.";
  const answer = createAnswer("s");
  for (let end = 1; end <= text.length; end++) {
    const views = answer.update(text.slice(0, end), true);
    const shown = htmlOf(views)
      .replace(/<[^>]+>/g, "")
      .replace(/&#x3C;/g, "<");
    const words = text
      .slice(0, end)
      .replace(/<\/?[a-z]*>?/g, " ")
      .split(/\s+/);
    // Every word of plain text that has fully arrived (a space after it) shows.
    for (const word of words.slice(0, -1))
      if (word) expect(shown, text.slice(0, end)).toContain(word);
  }
  expect(html(text)).toBe(
    "<p>Fee <sup>2</sup> due <mark>now</mark>, <kbd>Ctrl</kbd> and <u>x &#x3C;s>y</u> z&#x3C;/s> end.</p>",
  );
});

test("a key the answer names is announced: the design system's key cap without aria-hidden", () => {
  const markup = createAnswer("t")
    .update("Press <kbd>Esc</kbd>", false)
    .map((view) => renderToStaticMarkup(toReact(shownHast(view))))
    .join("");
  expect(markup).toMatch(/<kbd class="[^"]*kbd-cap[^"]*">Esc<\/kbd>/);
  expect(markup).not.toContain("aria-hidden");
});

test("a tag cut at a line's start (nesting.ts) is a tag only where micromark reads one in the source", () => {
  // Decoded, `&#62;` and `\"` read as a tag; micromark reads the source, where neither is one.
  expect(html("para\n<script &#62;x</script> y")).toContain("&#x3C;script >x&#x3C;/script> y");
  expect(html('para\n<pre a=\\"b\\">kept')).toBe('<p>para&#x3C;pre a="b">kept</p>');
  // As written, it is a tag there as anywhere else, and two on one line are both seen.
  expect(html("para\n<script>x</script> y")).toBe("<p>parax y</p>");
  expect(html('para\n<pre a="b">kept</pre> and\n<pre>two</pre>')).toBe("<p>parakept andtwo</p>");
  // A tag in the source whose text the value shows decoded stays text, whole (no offset of one is used
  // in the other).
  expect(html('para\n<pre a="&amp;">kept</pre> x')).toBe(
    '<p>para&#x3C;pre a="&#x26;">kept&#x3C;/pre> x</p>',
  );
  // In a note, whose positions are its block's.
  const note = html("A[^n].\n\n[^n]: B\n<script &#62;x</script> y");
  expect(note).toContain("&#x3C;script >x&#x3C;/script> y");
});

test("a closing tag at the end of a note pairs as it does in the body (the back-references' space is apart)", () => {
  const markup = html("A[^n].\n\n[^n]: Note <script>y</script>");
  expect(markup).not.toContain("script");
  expect(markup).toMatch(/Note y <a [^>]*data-footnote-backref/);
  expect(html("A[^n].\n\n[^n]: Note <sup>y</sup>")).toMatch(
    /Note <sup>y<\/sup> <a [^>]*data-footnote-backref/,
  );
});

test("inside a dropped never-markup pair, an image shows its alt text, and a reference's note has no way back", () => {
  expect(html("<script>![the alt](x.png)</script>")).toBe("<p>the alt</p>");
  const markup = html("See <script>x[^n]</script> and[^m].\n\n[^n]: Note n.\n\n[^m]: Note m.");
  expect(markup).toContain("<p>See x1 and<sup>");
  // Note n keeps no back-reference (its reference is text now), note m keeps its own.
  expect(markup).toMatch(/Note n\.\s*<\/p>/);
  expect(markup).toMatch(/Note m\. <a [^>]*data-footnote-backref/);
});
