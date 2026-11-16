// @vitest-environment happy-dom
// The Folder row with a very long path: the chosen folder and "Use a new folder in" each cut the path in its middle (its
// parent and its last part as parts of their own, so the parent gives way and the folder's own name stays), with the
// whole path in the text, for a screen reader, and under the pointer; and matter-form.css, read as text, keeps the row
// inside the form: one column no wider than the form, the path giving way, Choose folder… never shrinking. Happy-dom
// can't measure; the probe's ?scan=long-path is where the widths are measured.
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, test } from "vitest";
import { FolderRow } from "./matter-form-folder.tsx";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const LONG =
  "~/Documents/Clauseo/Matters/2026/Meridian Agro Traders Pvt Ltd v Kestrel Finance Ltd/Correspondence and pleadings/Interim";
const PLACE =
  "~/Documents/Clauseo/Matters/2026/Commercial disputes before the Bombay High Court/Original side/New matters";

let host: HTMLDivElement | null = null;
let root: Root | null = null;

afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  root = null;
});

async function mountRow() {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () =>
    root?.render(
      <FolderRow
        shown={LONG}
        made={false}
        offer={PLACE}
        picking={false}
        onChoose={() => {}}
        onOffer={() => {}}
        testIds={{ choose: "choose", offer: "offer" }}
      />,
    ),
  );
}

const parts = (el: Element | null | undefined) => ({
  head: el?.querySelector(".folder-path-head")?.textContent,
  tail: el?.querySelector(".folder-path-tail")?.textContent,
});

test("a long chosen folder: its parent and its own name apart, the whole path in the text and the title", async () => {
  await mountRow();
  const field = document.querySelector(".matter-form-folder-field");
  expect(parts(field)).toEqual({
    head: "~/Documents/Clauseo/Matters/2026/Meridian Agro Traders Pvt Ltd v Kestrel Finance Ltd/Correspondence and pleadings/",
    tail: "Interim",
  });
  expect(field?.getAttribute("title")).toBe(LONG);
  expect(document.querySelector('[data-testid="matter-folder-path"]')?.textContent).toBe(LONG);
  // Choose folder… is described by the path, whole
  const choose = document.querySelector('[data-testid="choose"]');
  const described = choose?.getAttribute("aria-describedby") ?? "";
  expect(document.getElementById(described)?.textContent).toBe(LONG);
});

test("a long place for new matters: the words whole, the path cut as the row's is, and whole in the name and title", async () => {
  await mountRow();
  const offer = document.querySelector('[data-testid="offer"]');
  expect(offer?.querySelector(".matter-form-offer-words")?.textContent).toBe(
    "Use a new folder in ",
  );
  expect(parts(offer?.querySelector(".matter-form-offer-path"))).toEqual({
    head: "~/Documents/Clauseo/Matters/2026/Commercial disputes before the Bombay High Court/Original side/",
    tail: "New matters",
  });
  // the name is the words with the path isolated, as before it could be cut short
  expect(offer?.textContent).toBe(`Use a new folder in \u2068${PLACE}\u2069`);
  expect(offer?.getAttribute("title")).toBe(PLACE);
});

// happy-dom gives import.meta.url an http scheme, so the sheet is read by the file's own folder
const css = readFileSync(join(import.meta.dirname, "matter-form.css"), "utf8").replace(
  /\/\*[\s\S]*?\*\//g,
  "",
);
/** The declarations of the rule whose selector is exactly `selector`. */
function decls(selector: string): Map<string, string> {
  const out = new Map<string, string>();
  for (const m of css.matchAll(/([^{}]+)\{([^{}]*)\}/g)) {
    if ((m[1] ?? "").replace(/\s+/g, " ").trim() !== selector) continue;
    for (const d of (m[2] ?? "").split(";")) {
      const at = d.indexOf(":");
      if (at > 0) out.set(d.slice(0, at).trim(), d.slice(at + 1).trim());
    }
  }
  return out;
}

test("the row stays inside the form: one column no wider than it, the paths give way, Choose folder… never does", () => {
  // an auto column would take the one-line width of the longest path and push the row past the dialog's edge
  expect(decls(".matter-form-folder").get("grid-template-columns")).toBe("minmax(0, 1fr)");
  expect(decls(".matter-form-folder-field").get("min-width")).toBe("0");
  expect(decls(".matter-form-folder-path").get("min-width")).toBe("0");
  expect(decls(".matter-form-choose").get("flex")).toBe("none");
  expect(decls(".matter-form-offer").get("min-width")).toBe("0");
  expect(decls(".matter-form-offer").get("max-width")).toBe("100%");
  expect(decls(".matter-form-offer-line").get("min-width")).toBe("0");
  expect(decls(".matter-form-offer-words").get("flex")).toBe("none");
  expect(decls(".matter-form-offer-path").get("min-width")).toBe("0");
  // nothing hides the overflow on the way: the paths are cut by their own ellipsis
  expect(css).not.toMatch(/overflow-x\s*:\s*hidden/);
});
