// G4's raw-syntax scan (clauseo-acp-private at 61a2bef, measurements/g4-markdown/spike/fidelity/raw.ts),
// typed: the raw Markdown syntax on screen in a rendered answer, one home for the vitest suites here and the
// private real-engine harness. The kinds are legal-fixture.ts RAW_KINDS.
import { RAW_KINDS } from "./legal-fixture.ts";

export type RawScan = { counts: Record<string, number>; samples: Record<string, string> };

const SUBSTRING_KINDS = RAW_KINDS.filter((kind) => !kind.startsWith("lead "));
const LEAD: [string, RegExp][] = [
  ["lead #", /^#/],
  ["lead >", /^>/],
  ["lead -", /^-(?:[\s-]|$)/],
];
const BLOCKS = "p,li,h1,h2,h3,h4,h5,h6,td,th";

/** The answer's text, without the link component's host hint (which is ours, not the answer's). */
export function answerText(root: Element): string {
  let text = "";
  const visit = (node: Node) => {
    if (node.nodeType === 3) text += node.nodeValue ?? "";
    else if (!(node instanceof Element && node.getAttribute("data-clauseo") === "link-host"))
      for (const child of node.childNodes) visit(child);
  };
  visit(root);
  return text;
}

function count(text: string, token: string): number {
  let n = 0;
  for (let at = text.indexOf(token); at >= 0; at = text.indexOf(token, at + token.length)) n++;
  return n;
}

/** Own leading text of a block element: its first text, skipping nothing but whitespace. */
function leading(el: Element): string {
  const first = el.firstChild;
  if (first && first.nodeType === 3) return (first.nodeValue ?? "").trimStart();
  if (el.tagName === "LI" && el.firstElementChild?.tagName === "P") return "";
  return (el.textContent ?? "").trimStart();
}

/** The raw Markdown syntax on screen in `root` (RAW_KINDS), with a sample of each at its last place. */
export function scanRaw(root: Element, withSamples = true): RawScan {
  const text = answerText(root);
  const counts: Record<string, number> = {};
  const samples: Record<string, string> = {};
  for (const kind of SUBSTRING_KINDS) {
    const n = count(text, kind);
    if (n === 0) continue;
    counts[kind] = n;
    if (withSamples) {
      const at = text.lastIndexOf(kind);
      samples[kind] = text.slice(Math.max(0, at - 30), at + 30);
    }
  }
  for (const el of root.querySelectorAll(BLOCKS)) {
    const lead = leading(el);
    for (const [kind, re] of LEAD) {
      if (!re.test(lead)) continue;
      counts[kind] = (counts[kind] ?? 0) + 1;
      if (withSamples) samples[kind] ??= `<${el.tagName.toLowerCase()}> ${lead.slice(0, 50)}`;
    }
  }
  return { counts, samples };
}
