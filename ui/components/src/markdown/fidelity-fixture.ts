// G4's fidelity checks (clauseo-acp-private at 61a2bef, measurements/g4-markdown/spike/fidelity/check.ts),
// applied to a rendered answer: one home for the vitest suites here (happy-dom) and the private
// real-engine harness. `checkAll` gives one result per spec entry (legal-fixture.ts SPEC); the raw-syntax
// scan is raw-scan-fixture.ts. Changes from the spike: typed, the document is the root's own, and a
// table's column alignment is read from the computed style only with `layout: true` (a real engine);
// without layout it is read from the cells' inline style or `align` attribute, and the result says so.
import { SPEC } from "./legal-fixture.ts";
import type { Expect, SpecEntry } from "./legal-fixture.ts";

export type CheckResult = {
  id: string;
  feature: string;
  script: string;
  pass: boolean;
  missing: boolean;
  failures: string[];
  components: string[];
  nfcOnly: boolean;
  /** Checks this run could not make without layout (computed text-align). */
  needsLayout: string[];
};

const norm = (s: string | null | undefined) =>
  (s ?? "").normalize("NFC").replace(/\s+/g, " ").trim();
const rawNorm = (s: string | null | undefined) => (s ?? "").replace(/\s+/g, " ").trim();

/** Text of an element without nested lists (a list item's own text). */
function ownText(el: Element): string {
  const copy = el.cloneNode(true) as Element;
  for (const list of copy.querySelectorAll("ul,ol")) list.remove();
  return copy.textContent ?? "";
}

class Ctx {
  failures: string[] = [];
  components = new Set<string>();
  needsLayout: string[] = [];
  missing = false;
  nfcOnly = false;
  /** Exact text after NFC; records when the raw code points differ. */
  text(label: string, actual: string | null | undefined, expected: string) {
    if (norm(actual) !== norm(expected)) {
      this.failures.push(
        `${label}: got "${norm(actual).slice(0, 120)}", expected "${norm(expected).slice(0, 120)}"`,
      );
      return false;
    }
    if (rawNorm(actual) !== rawNorm(expected)) this.nfcOnly = true;
    return true;
  }
  fail(message: string, missing = false) {
    this.failures.push(message);
    if (missing) this.missing = true;
  }
}

function all<T extends Element = Element>(root: ParentNode, selector: string): T[] {
  return Array.from(root.querySelectorAll<T>(selector));
}

function find<T extends Element = Element>(
  root: ParentNode,
  selector: string,
  test: (el: T) => boolean,
): T | undefined {
  return all<T>(root, selector).find(test);
}

/** Elements matching selector in document order, with the given text. */
function byText(root: Element, selector: string, text: string) {
  return find(root, selector, (el) => norm(el.textContent) === norm(text));
}

const byId = (root: Element, href: string) => {
  try {
    return root.ownerDocument.getElementById(decodeURIComponent(href.slice(1)));
  } catch {
    return null;
  }
};

const ALIGNS: Record<string, string> = {
  start: "left",
  end: "right",
  "-webkit-left": "left",
  "-webkit-right": "right",
  "-webkit-center": "center",
};

function checkTable(
  root: Element,
  expect: Extract<Expect, { kind: "table" }>,
  x: Ctx,
  layout: boolean,
) {
  const table = find(root, "table", (t) => {
    const headers = all(t, "thead th");
    return (
      headers.length === expect.header.length &&
      headers.every((th, i) => norm(th.textContent) === norm(expect.header[i]))
    );
  });
  if (!table) {
    const loose = find(
      root,
      "table",
      (t) => norm(t.querySelector("th")?.textContent) === norm(expect.header[0]),
    );
    x.fail(
      loose
        ? `table found but header differs: ${all(loose, "th")
            .map((th) => norm(th.textContent))
            .join(" / ")}`
        : "no <table> with this header",
      !loose,
    );
    return;
  }
  const headers = all(table, "thead th");
  const rows = all(table, "tbody tr");
  if (rows.length !== expect.rows.length)
    x.fail(`${rows.length} body rows, expected ${expect.rows.length}`);
  rows.forEach((tr, r) => {
    const cells = all(tr, "td");
    if (cells.length !== expect.header.length)
      x.fail(`row ${r + 1}: ${cells.length} cells, expected ${expect.header.length}`);
    cells.forEach((td, c) =>
      x.text(`row ${r + 1} cell ${c + 1}`, td.textContent, expect.rows[r]?.[c] ?? ""),
    );
  });
  if (!expect.align) return;
  if (!layout)
    x.needsLayout.push(
      "computed text-align of each column (checked: inline style or align attribute)",
    );
  expect.align.forEach((want, c) => {
    const cells = [headers[c], ...rows.map((tr) => all(tr, "td")[c])].filter(
      (cell): cell is Element => cell !== undefined,
    );
    for (const cell of cells) {
      let got = layout
        ? getComputedStyle(cell).textAlign
        : (cell as HTMLElement).style.textAlign || (cell.getAttribute("align") ?? "start");
      got = ALIGNS[got] ?? got;
      if (got !== want) {
        x.fail(
          `column ${c + 1}: text-align ${got}, expected ${want} (on <${cell.tagName.toLowerCase()}>)`,
        );
        break;
      }
    }
  });
}

function checkLists(root: Element, expect: Expect, x: Ctx) {
  if (expect.kind === "ordered-start") {
    const ol = find(
      root,
      "ol",
      (el) => norm(ownText(el.querySelector("li") ?? el)) === norm(expect.items[0]),
    );
    if (!ol) return x.fail("no <ol> whose first item has the text", true);
    if (ol.getAttribute("start") !== String(expect.start))
      x.fail(
        `start attribute ${JSON.stringify(ol.getAttribute("start"))}, expected "${expect.start}"`,
      );
    const items = Array.from(ol.children).filter((el) => el.tagName === "LI");
    if (items.length !== expect.items.length)
      x.fail(`${items.length} items, expected ${expect.items.length}`);
    items.forEach((li, i) => x.text(`item ${i + 1}`, ownText(li), expect.items[i] ?? ""));
  } else if (expect.kind === "nested") {
    let list: Element | undefined = find(
      root,
      expect.types[0]!,
      (el) => norm(ownText(el.querySelector("li") ?? el)) === norm(expect.levels[0]),
    );
    if (!list) return x.fail("no top-level list with the first item's text", true);
    const top = list;
    let depth = 0;
    for (let level = 0; level < expect.types.length; level++) {
      if (!list || list.tagName.toLowerCase() !== expect.types[level]) {
        x.fail(
          `level ${level + 1}: ${list ? `<${list.tagName.toLowerCase()}>` : "no list"}, expected <${expect.types[level]}>`,
        );
        break;
      }
      depth++;
      const li: Element | undefined = Array.from(list.children).find((el) => el.tagName === "LI");
      if (!li) {
        x.fail(`level ${level + 1}: no <li>`);
        break;
      }
      x.text(`level ${level + 1} text`, ownText(li), expect.levels[level]!);
      list = Array.from(li.children).find((el) => el.tagName === "UL" || el.tagName === "OL");
    }
    if (depth !== 4) x.fail(`nesting depth ${depth}, expected 4`);
    const second = Array.from(top.children).filter((el) => el.tagName === "LI")[1];
    x.text("second top-level item", second ? ownText(second) : "", expect.secondTopItem);
  } else if (expect.kind === "clause-paragraphs") {
    for (const text of expect.texts) {
      const p = byText(root, "p", text);
      if (!p) {
        const inList = find(root, "li", (li) =>
          norm(li.textContent).endsWith(norm(text).replace(/^\S+\s/, "")),
        );
        x.fail(
          inList
            ? `"${text.slice(0, 12)}…" became a list item`
            : `no <p> with "${text.slice(0, 24)}…"`,
          !inList,
        );
      } else if (p.closest("li,ol,ul")) x.fail(`"${text.slice(0, 12)}…" is inside a list`);
    }
  } else if (expect.kind === "clause-lists") {
    const first = (items: string[]) => (el: Element) =>
      norm(ownText(el.querySelector("li") ?? el)) === norm(items[0]);
    const ul = find(root, "ul", first(expect.bullet));
    if (!ul) x.fail("no <ul> whose first item is the first clause", true);
    else
      Array.from(ul.children).forEach((li, i) =>
        x.text(`bullet ${i + 1}`, ownText(li), expect.bullet[i] ?? "(extra item)"),
      );
    const ol = find(root, "ol", first(expect.ordered));
    if (!ol) x.fail("no <ol> whose first item is the first clause", true);
    else {
      Array.from(ol.children).forEach((li, i) =>
        x.text(`ordered ${i + 1}`, ownText(li), expect.ordered[i] ?? "(extra item)"),
      );
      const start = ol.getAttribute("start");
      if (start !== null && start !== "1") x.fail(`ordered list start ${start}, expected 1`);
    }
  }
}

function checkInline(root: Element, expect: Extract<Expect, { kind: "inline" }>, x: Ctx) {
  const near = () => find(root, "p", (el) => norm(el.textContent).includes(norm(expect.bold)));
  const p = byText(root, "p", expect.paragraph);
  if (!p) {
    const other = near();
    x.fail(
      other
        ? `paragraph text differs: "${norm(other.textContent).slice(0, 160)}"`
        : "no <p> with the text",
      !other,
    );
    if (!other) return;
  }
  const para = p ?? near()!;
  const is = (text: string) => (el: Element) => norm(el.textContent) === norm(text);
  if (
    !all(para, "strong").find(
      (el) => !el.querySelector("em") && !el.closest("em") && is(expect.bold)(el),
    )
  )
    x.fail("no <strong> with the bold text");
  if (
    !all(para, "em").find(
      (el) => !el.querySelector("strong") && !el.closest("strong") && is(expect.italic)(el),
    )
  )
    x.fail("no <em> with the italic text");
  if (!all(para, "strong em, em strong").find(is(expect.boldItalic)))
    x.fail("no <strong> and <em> nested around the bold-italic text");
  if (!all(para, "del,s").find(is(expect.struck))) x.fail("no <del> with the struck text");
  if (expect.intraword) {
    const { stem, suffix } = expect.intraword;
    if (!all(para, "strong").find(is(stem))) x.fail(`intraword: no <strong> with "${stem}"`);
    else if (!norm(para.textContent).includes(norm(stem + suffix)))
      x.fail("intraword: stem and suffix not joined");
  }
}

function checkFootnote(root: Element, expect: Extract<Expect, { kind: "footnote" }>, x: Ctx) {
  const p = find(root, "p", (el) => norm(el.textContent).startsWith(norm(expect.sentence)));
  if (!p) return x.fail("no <p> with the sentence", true);
  const ref = p.querySelector("sup a, a[data-footnote-ref]");
  if (!ref)
    return x.fail(
      `no footnote reference element; paragraph reads "${norm(p.textContent).slice(-40)}"`,
      true,
    );
  x.components.add(ref.getAttribute("data-clauseo") ?? "none");
  x.text("reference number", ref.textContent, expect.number);
  const href = ref.getAttribute("href") ?? "";
  const target = href.startsWith("#") ? byId(root, href) : null;
  if (!target) return x.fail(`reference href ${JSON.stringify(href)} points at no element`);
  if (!target.closest("section, [data-footnotes]"))
    x.fail("reference target is not in a footnotes section");
  if (!norm(target.textContent).includes(norm(expect.definition)))
    x.fail(`reference target text "${norm(target.textContent).slice(0, 80)}" lacks the definition`);
  const back = target.querySelector("a[data-footnote-backref], a[href^='#']");
  const backTarget = back ? byId(root, back.getAttribute("href") ?? "") : null;
  if (!back) x.fail("definition has no back-reference link");
  else if (backTarget !== ref && !backTarget?.contains(ref) && !ref.contains(backTarget ?? root))
    x.fail(`back-reference ${back.getAttribute("href")} does not point at the reference`);
}

function checkLinks(root: Element, expect: Expect, x: Ctx) {
  if (expect.kind === "links") {
    const p = byText(root, "p", expect.paragraph);
    if (!p) x.fail("no <p> with the exact text");
    for (const link of expect.links) {
      const a = all(p ?? root, "a").find((el) => el.getAttribute("href") === link.href);
      if (!a) {
        x.fail(`no <a href="${link.href.slice(0, 50)}">`, true);
        continue;
      }
      x.components.add(a.getAttribute("data-clauseo") ?? "none");
      if (a.getAttribute("data-clauseo") !== "link")
        x.fail(`<a href="${link.href.slice(0, 30)}"> not from our link component`);
      const own = Array.from(a.childNodes)
        .filter((n) => !(n instanceof Element && n.getAttribute("data-clauseo") === "link-host"))
        .map((n) => n.textContent)
        .join("");
      x.text(`link text (${link.href.slice(0, 24)})`, own, link.text);
    }
  } else if (expect.kind === "images") {
    for (const image of expect.images) {
      // The data: image is the same pixel in every script, so the alt text picks the instance.
      const img =
        all(root, "img").find(
          (el) =>
            el.getAttribute("src") === image.src &&
            norm(el.getAttribute("alt")) === norm(image.alt),
        ) ??
        all(root, "img").find(
          (el) => el.getAttribute("src") === image.src && !image.src.startsWith("data:"),
        );
      if (!img) {
        const placeholder = all(root, '[data-clauseo="image"][data-blocked]').find(
          (el) => norm(el.getAttribute("aria-label")) === norm(image.alt),
        );
        x.fail(
          placeholder
            ? `rendered as the blocked-image placeholder (${norm(placeholder.textContent)})`
            : `no <img src="${image.src.slice(0, 40)}">`,
          !placeholder,
        );
        continue;
      }
      x.components.add(img.getAttribute("data-clauseo") ?? "none");
      if (img.getAttribute("data-clauseo") !== "image")
        x.fail("<img> not from our image component");
      x.text("alt", img.getAttribute("alt"), image.alt);
    }
  } else if (expect.kind === "references") {
    if (!byText(root, "p", expect.paragraph)) x.fail("no <p> with the exact text");
    for (const ref of expect.refs) {
      const el = all(root, `[data-clauseo="${ref.component}"]`).find(
        (candidate) => candidate.getAttribute("data-ref") === ref.ref,
      );
      if (!el) {
        const asLink = all(root, "a").find((a) => norm(a.textContent) === norm(ref.text));
        x.fail(
          asLink
            ? `rendered as <a href=${JSON.stringify(asLink.getAttribute("href"))}>, not our ${ref.component} component`
            : `no ${ref.component} component for ${ref.ref}`,
          !asLink,
        );
        continue;
      }
      x.components.add(ref.component);
      x.text(`${ref.component} text`, el.textContent, ref.text);
    }
    if (root.querySelector('a[href^="clauseo-cite:"], a[href^="clauseo-paper:"]'))
      x.fail("a clauseo-cite: or clauseo-paper: URL reached an <a href>");
  }
}

function check(root: Element, expect: Expect, x: Ctx, layout: boolean): void {
  switch (expect.kind) {
    case "heading": {
      if (byText(root, `h${expect.level}`, expect.text)) return;
      const other = byText(root, "h1,h2,h3,h4,h5,h6,p", expect.text);
      x.fail(
        other
          ? `rendered as <${other.tagName.toLowerCase()}>, expected <h${expect.level}>`
          : `no <h${expect.level}> with the text`,
        !other,
      );
      return;
    }
    case "table":
      return checkTable(root, expect, x, layout);
    case "blockquote": {
      const outer = find(
        root,
        "blockquote",
        (bq) =>
          !bq.parentElement?.closest("blockquote") &&
          norm(bq.querySelector("p")?.textContent) === norm(expect.outer[0]),
      );
      if (!outer) return x.fail("no top-level <blockquote> starting with the text", true);
      const inner = Array.from(outer.children).find((el) => el.tagName === "BLOCKQUOTE");
      if (!inner) x.fail("no nested <blockquote> directly inside");
      else x.text("nested quote", inner.textContent, expect.inner);
      const ps = Array.from(outer.children).filter((el) => el.tagName === "P");
      x.text("outer first paragraph", ps[0]?.textContent, expect.outer[0] ?? "");
      x.text("outer second paragraph", ps[1]?.textContent, expect.outer[1] ?? "");
      if (inner?.querySelector("blockquote")) x.fail("nesting deeper than 2");
      return;
    }
    case "inline":
      return checkInline(root, expect, x);
    case "hr": {
      const ordered = all(root, "p,hr");
      const at = ordered.findIndex(
        (el) => el.tagName === "P" && norm(el.textContent) === norm(expect.before),
      );
      if (at < 0) return x.fail("paragraph before the rule not found", true);
      const next = ordered[at + 1];
      if (next?.tagName !== "HR")
        x.fail(
          `after the paragraph: ${next ? `<${next.tagName.toLowerCase()}> "${norm(next.textContent).slice(0, 40)}"` : "nothing"}, expected <hr>`,
          true,
        );
      else x.text("paragraph after the rule", ordered[at + 2]?.textContent, expect.after);
      return;
    }
    case "breaks": {
      const p = find(root, "p", (el) => norm(el.textContent).startsWith(norm(expect.lines[0])));
      if (!p) return x.fail("no <p> starting with the first line", true);
      const brs = p.querySelectorAll("br").length;
      if (brs !== expect.lines.length - 1)
        x.fail(`${brs} <br>, expected ${expect.lines.length - 1}`);
      const segments: string[] = [""];
      for (const node of p.childNodes) {
        if (node.nodeName === "BR") segments.push("");
        else segments[segments.length - 1] += node.textContent ?? "";
      }
      expect.lines.forEach((line, i) => x.text(`line ${i + 1}`, segments[i], line));
      return;
    }
    case "footnote":
      return checkFootnote(root, expect, x);
    case "literal": {
      if (byText(root, "p", expect.text)) return;
      const near = find(root, "p", (el) => norm(el.textContent).startsWith("Literal characters"));
      x.fail(
        near ? `text differs: "${norm(near.textContent)}"` : "no <p> with the literal text",
        !near,
      );
      return;
    }
    default:
      return expect.kind === "links" || expect.kind === "images" || expect.kind === "references"
        ? checkLinks(root, expect, x)
        : checkLists(root, expect, x);
  }
}

/** Every spec entry applied to `root`, a rendered answer. `layout`: the engine computes styles. */
export function checkAll(
  root: Element,
  { layout = false, spec = SPEC }: { layout?: boolean; spec?: readonly SpecEntry[] } = {},
): CheckResult[] {
  return spec.map((entry) => {
    const x = new Ctx();
    try {
      check(root, entry.expect, x, layout);
    } catch (error) {
      x.fail(`checker threw: ${String(error)}`);
    }
    return {
      id: entry.id,
      feature: entry.feature,
      script: entry.script,
      pass: x.failures.length === 0,
      missing: x.missing,
      failures: x.failures,
      components: [...x.components],
      nfcOnly: x.nfcOnly,
      needsLayout: x.needsLayout,
    };
  });
}
