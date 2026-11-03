// @vitest-environment happy-dom
// PaperTile for a file known only by its name: without a numeral it shows its title alone, and uncited it has no chip
// slot. With a numeral and a role it draws both, as before.
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
