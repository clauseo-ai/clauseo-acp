// The checks of the legal extras (legal-extras-fixture.ts) on a rendered answer: one result per row of
// EXTRAS_SPEC, and the raw syntax and citation scans, for the vitest suites here (happy-dom) and the private
// real-engine harness. Whether a long cell wraps and a wide table scrolls needs layout: here only the DOM
// that makes it so (the table in its own box) is checked.
import { answerText } from "./raw-scan-fixture.ts";
import { CITATION_FORMS, EXTRA_RAW, EXTRAS_SPEC, LITERALS, TAGS } from "./legal-extras-fixture.ts";
import type { ExtraEntry, ExtraExpect } from "./legal-extras-fixture.ts";

export type ExtraResult = { id: string; feature: string; pass: boolean; failures: string[] };

const norm = (text: string | null | undefined) =>
  (text ?? "").normalize("NFC").replace(/\s+/g, " ").trim();

const all = (root: ParentNode, selector: string) => Array.from(root.querySelectorAll(selector));

function byText(root: Element, selector: string, text: string): Element | undefined {
  return all(root, selector).find((el) => norm(el.textContent) === norm(text));
}

/** The element a fragment href names, percent-decoded as the link component does. */
function target(root: Element, href: string | null): Element | null {
  if (!href?.startsWith("#")) return null;
  try {
    return root.ownerDocument.getElementById(decodeURIComponent(href.slice(1)));
  } catch {
    return null;
  }
}

/** Own text of a block quote: its direct paragraphs. */
const quoteText = (quote: Element) =>
  norm(
    all(quote, ":scope > p")
      .map((p) => p.textContent)
      .join(" "),
  );

function check(root: Element, expect: ExtraExpect, fail: (message: string) => void): void {
  switch (expect.kind) {
    case "phrasing": {
      const block = byText(root, "p,li,dd,dt,td", expect.paragraph);
      if (!block) return fail(`no block with the text "${expect.paragraph}"`);
      const marks = all(block, TAGS).map((el) => ({
        tag: el.tagName.toLowerCase(),
        text: norm(el.textContent),
      }));
      const want = expect.marks.map((mark) => ({ tag: mark.tag, text: norm(mark.text) }));
      if (JSON.stringify(marks) !== JSON.stringify(want))
        fail(`elements ${JSON.stringify(marks)}, expected ${JSON.stringify(want)}`);
      for (const el of all(block, TAGS))
        if (el.tagName !== "KBD" && el.getAttributeNames().length > 0)
          fail(`<${el.tagName.toLowerCase()}> has attributes ${el.getAttributeNames().join(", ")}`);
      return;
    }
    case "plain": {
      const p = byText(root, "p", expect.paragraph);
      if (!p) return fail(`no paragraph with the text "${expect.paragraph}"`);
      if (p.children.length > 0)
        fail(
          `elements inside: ${all(p, "*")
            .map((el) => el.tagName.toLowerCase())
            .join(", ")}`,
        );
      return;
    }
    case "footnote": {
      const p = byText(root, "p", expect.paragraph);
      if (!p) return fail(`no paragraph with the text "${expect.paragraph}"`);
      const ref = p.querySelector("sup > a");
      if (norm(ref?.textContent) !== expect.number)
        return fail(`reference "${norm(ref?.textContent)}", expected ${expect.number}`);
      const note = target(root, ref!.getAttribute("href"));
      if (!norm(note?.textContent).startsWith(norm(expect.note)))
        fail(`the reference's note reads "${norm(note?.textContent)}"`);
      return;
    }
    case "tasks": {
      const list = all(root, "ul.contains-task-list").find(
        (ul) => norm(ul.querySelector("li")?.textContent) === norm(expect.items[0]?.text),
      );
      if (!list) return fail("no task list with these items");
      const items = all(list, ":scope > li").map((li) => ({
        text: norm(li.textContent),
        checked: (li.querySelector("input[type=checkbox]") as HTMLInputElement | null)?.checked,
      }));
      if (JSON.stringify(items) !== JSON.stringify(expect.items))
        fail(`items ${JSON.stringify(items)}`);
      return;
    }
    case "definitions": {
      const list = all(root, "dl").find(
        (dl) => norm(dl.querySelector("dt")?.textContent) === norm(expect.entries[0]?.term),
      );
      if (!list) return fail("no definition list with this first term");
      const entries: { term: string; definitions: string[] }[] = [];
      for (const child of Array.from(list.children)) {
        if (child.tagName === "DT")
          entries.push({ term: norm(child.textContent), definitions: [] });
        else if (child.tagName === "DD") entries.at(-1)?.definitions.push(norm(child.textContent));
      }
      if (JSON.stringify(entries) !== JSON.stringify(expect.entries))
        fail(`entries ${JSON.stringify(entries)}`);
      return;
    }
    case "list-paragraphs": {
      const heading = byText(root, `h${expect.heading.level}`, expect.heading.text);
      if (!heading) return fail(`no h${expect.heading.level} "${expect.heading.text}"`);
      const list = heading.nextElementSibling;
      if (list?.tagName !== "OL") return fail("the heading is not followed by an ordered list");
      const items = all(list, ":scope > li").map((li) =>
        all(li, ":scope > p").map((p) => norm(p.textContent)),
      );
      if (JSON.stringify(items) !== JSON.stringify(expect.items))
        fail(`items ${JSON.stringify(items)}`);
      return;
    }
    case "quotes": {
      let quote = all(root, "blockquote").find((q) => quoteText(q) === norm(expect.levels[0]));
      for (const [level, text] of expect.levels.entries()) {
        if (!quote || quoteText(quote) !== norm(text))
          return fail(`level ${level + 1} is not a block quote reading "${text}"`);
        quote = quote.querySelector(":scope > blockquote") ?? undefined;
      }
      return;
    }
    case "anchors": {
      const scopes = new Set<string>();
      const found = new Map<string, Element>();
      for (const { level, text, slug } of expect.headings) {
        const heading = all(root, `h${level}`).find(
          (h) => norm(h.textContent) === norm(text) && h.id.endsWith(`-${slug}`),
        );
        if (!heading) {
          fail(`no h${level} "${text}" with the slug ${slug}`);
          continue;
        }
        const scope = heading.id.slice(0, -slug.length - 1);
        if (!scope.startsWith("user-content-")) fail(`id ${heading.id} is not prefixed`);
        scopes.add(scope);
        found.set(slug, heading);
      }
      if (scopes.size > 1) fail(`heading ids in more than one scope: ${[...scopes].join(", ")}`);
      for (const { text, slug } of expect.links) {
        const link = byText(root, "a", text);
        if (!link) fail(`no link "${text}"`);
        else if (!found.has(slug) || target(root, link.getAttribute("href")) !== found.get(slug))
          fail(
            `link "${text}" (${link.getAttribute("href")}) does not point at the ${slug} heading`,
          );
      }
      return;
    }
    case "table": {
      const table = all(root, "table").find(
        (t) =>
          JSON.stringify(all(t, "thead th").map((th) => norm(th.textContent))) ===
          JSON.stringify(expect.header.map(norm)),
      );
      if (!table) return fail("no table with this header");
      const rows = all(table, "tbody tr").map((tr) =>
        all(tr, "td").map((td) => norm(td.textContent)),
      );
      if (JSON.stringify(rows) !== JSON.stringify(expect.rows.map((row) => row.map(norm))))
        fail(`rows ${JSON.stringify(rows).slice(0, 200)}`);
      const align = all(table, "thead th").map(
        (th) => (th as HTMLElement).style.textAlign || th.getAttribute("align") || "",
      );
      if (JSON.stringify(align) !== JSON.stringify(expect.align))
        fail(`alignment ${JSON.stringify(align)}`);
      if (table.parentElement?.getAttribute("data-clauseo") !== "table")
        fail("the table is not in its own scroll box");
      return;
    }
    case "block-break": {
      const before = byText(root, "p", expect.before);
      const br = before?.nextElementSibling;
      if (br?.tagName !== "BR") return fail(`no line break after "${expect.before}"`);
      if (norm(br.nextElementSibling?.textContent) !== norm(expect.after))
        fail(`the line break is not followed by "${expect.after}"`);
      return;
    }
  }
}

/** One result per row of EXTRAS_SPEC for the answer rendered in `root`. */
export function checkExtras(root: Element): ExtraResult[] {
  return EXTRAS_SPEC.map((entry: ExtraEntry) => {
    const failures: string[] = [];
    check(root, entry.expect, (message) => failures.push(message));
    return { id: entry.id, feature: entry.feature, pass: failures.length === 0, failures };
  });
}

/**
 * The raw syntax of EXTRA_RAW on screen, outside the LITERALS paragraphs (as much of each as has arrived,
 * of `arrived`, the text so far): each token and how often.
 */
export function rawExtras(root: Element, arrived?: string): Record<string, number> {
  let text = answerText(root);
  for (const literal of LITERALS) {
    const at = (arrived ?? literal).indexOf(literal.slice(0, 8));
    const shown =
      arrived === undefined
        ? literal
        : literal.slice(0, Math.max(0, arrived.length - at)).trimEnd();
    // What shows of the literal so far: tail repair may still hide its last word.
    let part = shown;
    while (part !== "" && !text.includes(part))
      part = part.slice(0, Math.max(0, part.lastIndexOf(" ")));
    if (at >= 0 && part !== "") text = text.replace(part, "");
  }
  const counts: Record<string, number> = {};
  for (const token of EXTRA_RAW) {
    const n = text.split(token).length - 1;
    if (n > 0) counts[token] = n;
  }
  return counts;
}

/** Elements a citation must never be inside, or turned into. */
const MANGLED = "a,sup,sub,del,s,em,strong,mark,ins,u,li,code,h1,h2,h3,h4,h5,h6";
/** Parts of the citation forms that no other text of the extras answer holds. */
const CITATION_PARTS = ["SCC", "AIR 1950", "SCR 88", "OnLine Del"];

/**
 * How the citations of `arrived` (the text so far) show: every form that has fully arrived must show as
 * written, and no part of one may be inside a link, footnote, list, superscript, emphasis or strikethrough.
 */
export function citationFailures(root: Element, arrived: string): string[] {
  const failures: string[] = [];
  const text = norm(answerText(root));
  for (const form of CITATION_FORMS)
    if (arrived.includes(form) && !text.includes(form)) failures.push(`"${form}" not shown intact`);
  for (const el of all(root, MANGLED)) {
    const inside = norm(el.textContent);
    for (const part of CITATION_PARTS)
      if (inside.includes(part))
        failures.push(`"${part}" inside <${el.tagName.toLowerCase()}>: ${inside.slice(0, 60)}`);
  }
  return failures;
}
