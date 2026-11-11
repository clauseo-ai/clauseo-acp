// @vitest-environment happy-dom
// PaperTile for a file known only by its name: without a numeral it shows its title alone, and uncited it has no chip
// slot. With a numeral and a role it draws both, as before. Hindi names: a long Devanagari title stays whole on the
// face and in its tooltip, code point for code point (parts-css.test.ts pins that the wall wraps it, never clipping).
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, test } from "vitest";
import { PaperTile, type PaperTileProps } from "./paper-tile.tsx";
import { IconFilePdf, IconFileWord, IconFolder } from "../icons/icons.tsx";
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

test("a tile away from this computer says why on its face, does nothing, and says so to assistive technology", async () => {
  let turned = 0;
  await mount({
    id: "away-0",
    state: "uncited",
    title: "Lease deed.pdf",
    fileName: true,
    tip: "Not on this computer.",
    label: "Not on this computer.",
    away: "Only in OneDrive",
    onTurn: () => void turned++,
  });
  const tile = host?.querySelector("li.tile");
  expect(tile?.hasAttribute("data-away")).toBe(true);
  const face = host?.querySelector<HTMLButtonElement>(".tile-face");
  expect(face?.getAttribute("aria-disabled")).toBe("true");
  expect(face?.querySelector(".tile-cue")?.textContent).toBe("Only in OneDrive");
  const described = face?.getAttribute("aria-describedby") ?? "";
  expect(document.getElementById(described)?.textContent).toBe("Not on this computer.");
  // a DOM-safe id, never the path
  expect(tile?.id).toMatch(/paper-away-0$/);
  await act(async () => face?.click());
  expect(turned).toBe(0);
});

/* ---------- the file's colour ---------- */

test.each([
  ["word", "Reply.docx"],
  ["pdf", "Notice.pdf"],
  ["excel", "Ledger.xlsx"],
  ["powerpoint", "Outline.pptx"],
] as const)(
  "a %s file's tile carries its tone for the wall to colour its glyph's tab",
  async (tone, title) => {
    await mount({
      id: "f1",
      state: "uncited",
      title,
      fileName: true,
      fileIcon: IconFilePdf,
      fileTone: tone,
    });
    const tile = host?.querySelector("li.tile");
    expect(tile?.getAttribute("data-file")).toBe(tone);
    // the tab is its own path, so the colour lands on it and the page keeps its ink
    expect(tile?.querySelectorAll(".tile-glyph .file-badge")).toHaveLength(1);
  },
);

test("a neutral file, a tile without a tone and a tile without a glyph carry no colour", async () => {
  await mount({
    id: "f1",
    state: "uncited",
    title: "scan.jpg",
    fileIcon: IconFolder,
    fileTone: "neutral",
  });
  expect(host?.querySelector("li.tile")?.hasAttribute("data-file")).toBe(false);
  await act(async () => root?.unmount());
  host?.remove();
  await mount({ id: "f2", state: "uncited", title: "brief.txt", fileIcon: IconFilePdf });
  expect(host?.querySelector("li.tile")?.hasAttribute("data-file")).toBe(false);
  await act(async () => root?.unmount());
  host?.remove();
  await mount({ id: "f3", state: "uncited", title: "Reply.docx", fileTone: "word" });
  expect(host?.querySelector("li.tile")?.hasAttribute("data-file")).toBe(false);
});

test("a dimmed tile keeps its tone beside its dim, so the wall paints the tone's dimmed twin", async () => {
  await mount({
    id: "away-0",
    state: "uncited",
    title: "Old reply.docx",
    fileName: true,
    tip: "Not on this computer.",
    away: "Only in OneDrive",
    fileIcon: IconFileWord,
    fileTone: "word",
  });
  const tile = host?.querySelector("li.tile");
  expect(tile?.hasAttribute("data-away")).toBe(true);
  expect(tile?.getAttribute("data-file")).toBe("word");
  expect(tile?.matches('.tile[data-away][data-file="word"]')).toBe(true);
});
