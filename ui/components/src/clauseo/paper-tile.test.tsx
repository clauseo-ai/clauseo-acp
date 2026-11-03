// @vitest-environment happy-dom
// PaperTile for a file known only by its name: without a numeral it shows its title alone, and uncited it has no chip
// slot. With a numeral and a role it draws both, as before. Hindi names: a long Devanagari title stays whole on the
// face and in its tooltip, code point for code point (parts-css.test.ts pins that the wall wraps it, never clipping).
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, test } from "vitest";
import { PaperTile, type PaperTileProps } from "./paper-tile.tsx";
import { IconFilePdf } from "../icons/icons.tsx";
import { iconIn } from "../icons/icon-in.tsx";
import { RoomScopeProvider } from "./room-scope-provider.tsx";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement | null = null;
let root: Root | null = null;

async function mount(props: PaperTileProps) {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () =>
    root?.render(
      <RoomScopeProvider>
        <ol>
          <PaperTile {...props} />
        </ol>
      </RoomScopeProvider>,
    ),
  );
}

afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  root = null;
  host = null;
});

test("a tile without a numeral shows its title alone, and uncited it has no chip slot", async () => {
  await mount({ id: "f1", state: "uncited", title: "Notice to the bank.docx" });
  const face = host?.querySelector(".tile-face");
  expect(face?.querySelector(".tile-no")).toBeNull();
  expect(face?.querySelector(".tile-foot")).toBeNull();
  expect(face?.textContent).toBe("Notice to the bank.docx");
});

test("a tile with a numeral and a role keeps its page and its chip", async () => {
  await mount({ id: "p1", state: "relied", numeral: { from: 1, to: 4 }, title: "Notice" });
  const face = host?.querySelector(".tile-face");
  expect(face?.querySelector(".tile-no")?.textContent).toContain("1");
  expect(face?.querySelector(".tile-foot")).not.toBeNull();
});

test("given its file type's glyph, a tile shows it at 24 px on its top line, before the title, hidden", async () => {
  await mount({
    id: "f1",
    state: "uncited",
    title: "Notice to the bank.pdf",
    fileIcon: IconFilePdf,
  });
  const face = host?.querySelector(".tile-face");
  const svg = iconIn(face, IconFilePdf, 24, "tile-glyph");
  expect(svg?.getAttribute("aria-hidden")).toBe("true");
  expect(svg?.nextElementSibling?.classList.contains("tile-title")).toBe(true);
  // the name still names the tile
  expect(face?.textContent).toBe("Notice to the bank.pdf");
});

test("without a glyph a tile shows none, as the numbered tiles do", async () => {
  await mount({ id: "f1", state: "uncited", title: "Notice to the bank.pdf" });
  expect(host?.querySelector(".tile-face svg")).toBeNull();
});

/* ---------- Hindi names (Hindi audit 3b) ---------- */

/** A long name with conjuncts (प्र, त्त), a comma and Latin digits. */
const LONG_HI = "अनुबंध की प्रमाणित प्रति, धारा 138 के अंतर्गत नोटिस का उत्तर.pdf";
/** A nukta name, precomposed: क़ is U+0958, which NFC splits into क + ़ (a composition exclusion). */
const NUKTA_HI = "\u0958ानूनी नोटिस.docx";
/** A string's code points, so a failure shows which mark moved. */
const points = (s: string) => Array.from(s, (c) => c.codePointAt(0));

test.each([
  ["a long Devanagari name", LONG_HI],
  ["a nukta name as filed (U+0958)", NUKTA_HI],
  ["a nukta name in NFC (क + U+093C)", NUKTA_HI.normalize("NFC")],
])("%s shows whole on the face, code point for code point", async (_, title) => {
  await mount({ id: "f1", state: "uncited", title, fileIcon: IconFilePdf });
  const name = host?.querySelector(".tile-title");
  // never shortened, never normalised: the face is the name as the folder has it
  expect(name?.textContent).toBe(title);
  expect(points(name?.textContent ?? "")).toEqual(points(title));
  // a Devanagari name takes lang="hi" in the English interface too, so the Hindi type follows it (1f)
  expect(name?.getAttribute("lang")).toBe("hi");
});

test("a tile's hint carries the full Devanagari name as its tooltip, the comma and all", async () => {
  await mount({ id: "f1", state: "uncited", title: "अनुबंध की प्रमाणित प्रति", hint: LONG_HI });
  const face = host?.querySelector(".tile-face");
  expect(face?.getAttribute("title")).toBe(LONG_HI);
  expect(face?.querySelector(".tile-title")?.textContent).toBe("अनुबंध की प्रमाणित प्रति");
});

test("a tile's label carries the full Devanagari name for assistive technology", async () => {
  await mount({ id: "f1", state: "uncited", title: "क़ानूनी नोटिस", label: NUKTA_HI });
  const face = host?.querySelector(".tile-face");
  const more = host?.querySelector(`#${face?.getAttribute("aria-describedby") ?? "none"}`);
  expect(more?.textContent).toBe(NUKTA_HI);
});
