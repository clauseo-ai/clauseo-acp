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
import { EXTRA_FEATURES, EXTRAS_SPEC, LEGAL_EXTRAS } from "./legal-extras-fixture.ts";
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

test("the fixture has a row per feature in English, Hindi and Tamil", () => {
  for (const feature of Object.keys(EXTRA_FEATURES))
    expect(
      new Set(
        EXTRAS_SPEC.filter((entry) => entry.feature === feature).map((entry) => entry.script),
      ),
      feature,
    ).toEqual(new Set(["en", "hi", "ta"]));
});

test("the finished extras answer shows every row's expected DOM, and no raw syntax", () => {
  act(() => root.render(<></>));
  render(LEGAL_EXTRAS, false);
  const failures = checkExtras(host).filter((result) => !result.pass);
  expect(failures).toEqual([]);
  expect(rawExtras(host)).toEqual({});
  expect(citationFailures(host, LEGAL_EXTRAS)).toEqual([]);
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
