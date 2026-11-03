// @vitest-environment happy-dom
// The legal extras (legal-extras-fixture.ts) through the renderer in happy-dom: every row's expected DOM,
// written before the features, on the finished render; no raw syntax or allowlisted tag on screen,
// finished or at any 16-character delta of the stream; and the Indian case citations intact at every
// delta. Layout (a long cell wrapping, a wide table scrolling) is the private harness's.
import { act } from "react";
import { createRoot } from "react-dom/client";
import type { Root } from "react-dom/client";
import { afterAll, beforeAll, expect, test } from "vitest";
import { checkExtras, citationFailures, rawExtras } from "./legal-extras-check-fixture.ts";
import { EXTRA_FEATURES, EXTRAS_SPEC, HINDI_ONLY, LEGAL_EXTRAS } from "./legal-extras-fixture.ts";
import { Markdown } from "./markdown.tsx";

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

test("the fixture has a row per feature in English, Hindi and Tamil (Hindi only for HINDI_ONLY)", () => {
  for (const feature of Object.keys(EXTRA_FEATURES))
    expect(
      new Set(
        EXTRAS_SPEC.filter((entry) => entry.feature === feature).map((entry) => entry.script),
      ),
      feature,
    ).toEqual(new Set(HINDI_ONLY.includes(feature) ? ["hi"] : ["en", "hi", "ta"]));
});

test("the finished extras answer shows every row's expected DOM, and no raw syntax", () => {
  act(() => root.render(<></>));
  render(LEGAL_EXTRAS, false);
  const failures = checkExtras(host).filter((result) => !result.pass);
  expect(failures).toEqual([]);
  expect(rawExtras(host)).toEqual({});
  expect(citationFailures(host, LEGAL_EXTRAS)).toEqual([]);
});

const codePoints = (text: string) => Array.from(text, (char) => char.codePointAt(0));

test("Devanagari in a code block and in inline code keeps its exact code points, unescaped and unsplit", () => {
  act(() => root.render(<></>));
  render(LEGAL_EXTRAS, false);
  const spec = (id: string) => EXTRAS_SPEC.find((entry) => entry.id === id)!.expect;
  const block = spec("X13-code-hi");
  const inline = spec("X13-inline-hi");
  if (block.kind !== "code-block" || inline.kind !== "phrasing")
    throw new Error("X13 rows changed kind");
  const clause = block.text;
  const section = inline.marks[0]!.text;
  // The fixture strings are what the Markdown holds, in NFC, with every shaping case in them.
  for (const text of [clause, section]) {
    expect(LEGAL_EXTRAS).toContain(text);
    expect(text.normalize("NFC")).toBe(text);
  }
  const cases = ["क्ष", "त्र", "ज्ञ", "श्र", "र्म", "ज\u093C", "क\u093C", "क\u094D\u200Dष"];
  for (const part of cases) expect(clause, part).toContain(part);

  const pres = Array.from(host.querySelectorAll('pre[data-lang="text"]'));
  expect(pres).toHaveLength(1);
  const code = pres[0]!.querySelector("code")!;
  expect(code.textContent).toBe(clause);
  expect(codePoints(code.textContent!)).toEqual(codePoints(clause));
  expect(code.childNodes).toHaveLength(1);
  expect(code.firstChild!.nodeType).toBe(Node.TEXT_NODE);
  expect(code.innerHTML).not.toContain("&");

  const inlines = Array.from(host.querySelectorAll("p > code")).filter(
    (el) => el.textContent === section,
  );
  expect(inlines).toHaveLength(1);
  expect(codePoints(inlines[0]!.textContent!)).toEqual(codePoints("धारा १३८"));
  expect(inlines[0]!.childNodes).toHaveLength(1);
  expect(inlines[0]!.innerHTML).not.toContain("&");
});

test("streamed in 16-character deltas, no raw syntax or tag shows and every citation stays intact", () => {
  act(() => root.render(<></>));
  const seen: { length: number; problems: unknown; tail: string }[] = [];
  for (let end = 16; end < LEGAL_EXTRAS.length + 16; end += 16) {
    const text = LEGAL_EXTRAS.slice(0, end);
    render(text, true);
    const raw = rawExtras(host, text);
    const citations = citationFailures(host, text);
    if (Object.keys(raw).length > 0 || citations.length > 0)
      seen.push({ length: end, problems: { raw, citations }, tail: text.slice(-40) });
  }
  render(LEGAL_EXTRAS, false);
  expect(seen).toEqual([]);
}, 60_000);
