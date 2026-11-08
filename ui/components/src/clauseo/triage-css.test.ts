// triage.css (U16b), read as text with the rail's other sheets: nothing moves under the pointer or the focus (no rule
// that applies only then changes a row's box), nothing clips or fades a title at rest, under the pointer or with the
// focus, every row's buttons stand over its end on an opaque fill, the info icon's target is at least 24 px, and Undo is
// padded so its focus ring clears its letters.
import { readFileSync } from "node:fs";
import { expect, test } from "vitest";

const css = readFileSync(new URL("triage.css", import.meta.url), "utf8").replace(
  /\/\*[\s\S]*?\*\//g,
  "",
);

/** Every innermost rule: its selector and its declarations, by property. */
function rules(): { selector: string; decls: Map<string, string> }[] {
  const out: { selector: string; decls: Map<string, string> }[] = [];
  for (const m of css.matchAll(/([^{}]+)\{([^{}]*)\}/g)) {
    const decls = new Map<string, string>();
    for (const d of (m[2] ?? "").split(";")) {
      const at = d.indexOf(":");
      if (at > 0) decls.set(d.slice(0, at).trim(), d.slice(at + 1).trim());
    }
    out.push({ selector: (m[1] ?? "").replace(/\s+/g, " ").trim(), decls });
  }
  return out;
}

test("nothing moves when a row's buttons show: no rule for the pointer or the focus changes a box", () => {
  const box =
    /^(padding|margin|width|height|min-|max-|inset|top|left|right|bottom|font|line-height|display)/;
  const shown = rules().filter((r) => /:hover|:focus-within|data-popup-open/.test(r.selector));
  expect(shown.length).toBeGreaterThanOrEqual(2);
  const moving = shown.flatMap((r) =>
    [...r.decls.keys()].filter((p) => box.test(p)).map((p) => `${r.selector} { ${p} }`),
  );
  expect(moving).toEqual([]);
  // the buttons stand over the line's end, out of the flow
  const acts = rules().find((r) => r.selector === ".room-conv-acts");
  expect(acts?.decls.get("position")).toBe("absolute");
});

test("the info icon's target is the least target, 24 px", () => {
  const info = rules().find((r) => r.selector === ".room-info");
  expect(info?.decls.get("width")).toBe("var(--size-target)");
  expect(info?.decls.get("height")).toBe("var(--size-target)");
});

test("Undo is padded, so its focus ring clears its letters", () => {
  const undo = rules().find((r) => r.selector === ".room-line-undo");
  expect(undo?.decls.get("padding")).toBe("0 var(--s1)");
});

test("no title is clipped or faded, at rest, under the pointer or with the focus", () => {
  const clip =
    /mask|overflow\s*:\s*(hidden|clip)|text-overflow|line-clamp|clip-path|white-space\s*:\s*nowrap/;
  const title = /room-conv|room-shelf-row|room-matter-name|room-matter-note|room-one-act/;
  const found: string[] = [];
  for (const sheet of ["triage.css", "matters.css", "shell.css"]) {
    const text = readFileSync(new URL(sheet, import.meta.url), "utf8").replace(
      /\/\*[\s\S]*?\*\//g,
      "",
    );
    for (const m of text.matchAll(/([^{}]+)\{([^{}]*)\}/g))
      if (title.test(m[1] ?? "") && clip.test(m[2] ?? ""))
        found.push(`${sheet}: ${(m[1] ?? "").trim()}`);
  }
  expect(found).toEqual([]);
  // no row holds a button's width: every row's buttons stand over its end, out of the flow
  expect(
    rules().filter((r) => r.decls.get("padding-right")?.includes("--size-control-small")),
  ).toEqual([]);
});

test("a conversation's title takes the row up to its mark: its buttons stand over the row's end on an opaque fill", () => {
  // no width held for the buttons on a conversation's row
  expect(
    rules().filter((r) => /\.room-conv-item\[data-triage\] \.room-conv\b(?!-)/.test(r.selector)),
  ).toEqual([]);
  // the hovered row's tone over the plane it sits on, so no word shows through
  const fill = rules().find(
    (r) =>
      r.selector === ".room-conv-item[data-triage] .room-conv-acts, .room-done .room-conv-acts",
  );
  expect(fill?.decls.get("background")).toBe(
    "linear-gradient(var(--hover), var(--hover)), var(--tile-hover)",
  );
});

test("a done row's title takes the row: Mark as not done stands over its end on the hovered row's opaque tone", () => {
  expect(rules().filter((r) => r.selector.includes("room-one-act"))).toEqual([]);
  const fill = rules().find((r) => r.selector.split(", ").includes(".room-done .room-conv-acts"));
  expect(fill?.decls.get("background")).toBe(
    "linear-gradient(var(--hover), var(--hover)), var(--tile-hover)",
  );
});

test("a snoozed row's title takes the row: Wake now stands over its end on an opaque fill of the hovered ground", () => {
  expect(
    rules().filter((r) => r.selector.includes("room-shelf-row") && r.decls.has("padding-right")),
  ).toEqual([]);
  const fill = rules().find((r) => r.selector === ".room-shelf-rows .room-conv-acts");
  expect(fill?.decls.get("background")).toBe(
    "linear-gradient(var(--hover), var(--hover)), var(--ground)",
  );
});
