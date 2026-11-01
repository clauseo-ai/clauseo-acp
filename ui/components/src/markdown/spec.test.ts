import { describe, expect, test } from "vitest";
import spec from "commonmark-spec/spec.txt?raw";
import gfmSpec from "./gfm-spec.json";
import { EXCLUDED, REASONS } from "./spec-exclusions-fixture.ts";
import type { Exclusion } from "./spec-exclusions-fixture.ts";
import { loadEntities } from "./entities.ts";
import { chunkingsFor, examplesOf, normalise, streamed, withTabs } from "./spec-fixture.ts";

// The official suites, run through the streaming path: each example goes into one answer model in several
// chunkings (splitter, document-wide definitions, tail repair while streaming, sanitiser), and the HTML of
// the final hast, normalised (spec-fixture.ts), must equal the spec's. Excluded examples must still fail.
// The full entity table, which the renderer loads lazily (entities.ts), is loaded first.
await loadEntities();

const EXAMPLES = [
  ...examplesOf(spec).map((example) => ({ ...example, spec: "commonmark" as const })),
  ...gfmSpec.examples.map((example) => ({ ...withTabs(example), spec: "gfm" as const })),
];
const keyOf = (entry: { spec: string; example: number }) => `${entry.spec} ${entry.example}`;
const EXCLUSIONS = new Map<string, Exclusion>(EXCLUDED.map((entry) => [keyOf(entry), entry]));

test("the suites hold every example: 652 in CommonMark 0.31.2, 24 extension examples in GFM", () => {
  expect(EXAMPLES.filter((example) => example.spec === "commonmark")).toHaveLength(652);
  expect(
    EXAMPLES.filter((example) => example.spec === "gfm").map((example) => example.section),
  ).toEqual([
    ...Array<string>(8).fill("Tables (extension)"),
    ...Array<string>(2).fill("Task list items (extension)"),
    ...Array<string>(2).fill("Strikethrough (extension)"),
    ...Array<string>(11).fill("Autolinks (extension)"),
    "Disallowed Raw HTML (extension)",
  ]);
});

test("each exclusion names a real example once, with the spec's section and a known reason", () => {
  expect(EXCLUSIONS.size).toBe(EXCLUDED.length);
  for (const entry of EXCLUDED) {
    const example = EXAMPLES.find((candidate) => keyOf(candidate) === keyOf(entry));
    expect(example?.section, keyOf(entry)).toBe(entry.section);
    expect(Object.keys(REASONS)).toContain(entry.reason);
  }
});

test("the normalisation sees through markup differences but not content ones", () => {
  expect(normalise('<p>a <em>b</em></p>\n<img src="x" alt="y" title="t" />')).toBe(
    normalise('<p>a <em>b</em></p><img alt="y" src="x">'),
  );
  expect(normalise("<p>a\n  b</p>")).toBe(normalise("<p>a b</p>"));
  expect(normalise("<pre><code>a\n  b</code></pre>")).not.toBe(
    normalise("<pre><code>a b</code></pre>"),
  );
  expect(normalise("<p>a</p>")).not.toBe(normalise("<p>A</p>"));
  expect(normalise('<a href="/x">a</a>')).not.toBe(normalise("<a>a</a>"));
});

describe.each(["commonmark", "gfm"] as const)("%s spec", (name) => {
  test.for(EXAMPLES.filter((example) => example.spec === name))(
    "example $example ($section)",
    (example) => {
      const expected = normalise(example.html);
      const results = chunkingsFor(example.markdown).map((size) => ({
        size,
        html: normalise(streamed(example.markdown, size)),
      }));
      const exclusion = EXCLUSIONS.get(keyOf(example));
      if (exclusion) {
        // Still different (else the entry is stale), and the same whichever way it streamed.
        expect(
          results[0]?.html,
          `excluded (${exclusion.reason}) but now passes: remove it`,
        ).not.toBe(expected);
        for (const result of results)
          expect(result.html, `chunks of ${result.size}`).toBe(results[0]?.html);
        return;
      }
      for (const result of results) expect(result.html, `chunks of ${result.size}`).toBe(expected);
    },
  );
});
