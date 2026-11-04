// triage.css (U16b), read as text: nothing moves under the pointer or the focus (no rule that applies only then changes
// a row's box), the info icon's target is at least 24 px, and Undo is padded so its focus ring clears its letters.
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
  expect(shown.length).toBeGreaterThan(2);
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
