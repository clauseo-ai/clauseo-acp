// @vitest-environment happy-dom
// G4's fidelity spec and raw-syntax scan on the legal answer (legal-fixture.ts), in happy-dom: the finished
// render must pass every check a DOM can show (F12 footnotes included), and streaming the answer in
// 16-character deltas must never show raw Markdown syntax. Computed styles (a table's text-align) need a
// real engine: the private harness runs the same checks with layout there.
import { act } from "react";
import { createRoot } from "react-dom/client";
import type { Root } from "react-dom/client";
import { afterAll, beforeAll, expect, test } from "vitest";
import { checkAll } from "./fidelity-fixture.ts";
import { LEGAL, SPANS } from "./legal-fixture.ts";
import { Markdown } from "./markdown.tsx";
import { scanRaw } from "./raw-scan-fixture.ts";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

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

const render = (text: string, streaming: boolean) =>
  act(() => root.render(<Markdown text={text} streaming={streaming} />));

const literal = SPANS.literals[0]!;

/** How often `token` occurs in `text`, as raw.ts counts. */
function count(text: string, token: string): number {
  let n = 0;
  for (let at = text.indexOf(token); at >= 0; at = text.indexOf(token, at + token.length)) n++;
  return n;
}

/**
 * Raw syntax on screen once the literal paragraph's own characters are taken off: as much of the paragraph
 * as has arrived, its escapes resolved (a half-arrived escape shows nothing).
 */
function rawOnScreen(length: number): Record<string, number> {
  const { counts } = scanRaw(host, false);
  const arrived = LEGAL.slice(literal.start, Math.min(length, literal.end))
    .replace(/\\(.)/g, "$1")
    .replace(/\\$/, "");
  if (length > literal.start)
    for (const kind of Object.keys(literal.tokens))
      if (counts[kind] !== undefined)
        counts[kind] = Math.max(0, counts[kind] - count(arrived, kind));
  for (const kind of Object.keys(counts)) if (counts[kind] === 0) delete counts[kind];
  return counts;
}

test("the finished legal answer passes G4's fidelity spec, footnotes (F12) included", () => {
  act(() => root.render(<></>));
  render(LEGAL, false);
  const results = checkAll(host);
  const failures = results.filter((result) => !result.pass);
  // The matter's file images in the fixture are paths (`clauseo-file:matter-118/signature-page.png`), which
  // decision 0001 rules out (files are opaque ids): they render as the blocked-image placeholder.
  for (const failure of failures) {
    expect(failure.feature, JSON.stringify(failure)).toBe("F14");
    expect(failure.failures, failure.id).toEqual([
      expect.stringMatching(/^rendered as the blocked-image placeholder/),
    ]);
  }
  expect(results.filter((result) => result.feature === "F12" && result.pass)).toHaveLength(3);
  expect(results.filter((result) => result.pass).length).toBe(results.length - 3);
  expect(results.flatMap((result) => result.needsLayout)).toHaveLength(3);
  expect(rawOnScreen(LEGAL.length)).toEqual({});
});

test("streamed in 16-character deltas, the legal answer never shows raw syntax", () => {
  act(() => root.render(<></>));
  const seen: { length: number; counts: Record<string, number>; tail: string }[] = [];
  for (let end = 16; end < LEGAL.length; end += 16) {
    render(LEGAL.slice(0, end), true);
    const counts = rawOnScreen(end);
    if (Object.keys(counts).length > 0)
      seen.push({ length: end, counts, tail: LEGAL.slice(Math.max(0, end - 40), end) });
  }
  render(LEGAL, false);
  expect(seen).toEqual([]);
}, 60_000);
