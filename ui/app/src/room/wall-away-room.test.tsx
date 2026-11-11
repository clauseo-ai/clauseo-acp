// @vitest-environment happy-dom
// The wall's dimmed tiles in the room (paper-wall.tsx, wall-away.ts): files only online and folders Clauseo couldn't
// open, among the papers, opening nothing, each with its tip for assistive technology; the new conversation's count
// says how many more are only online. A matter whose files are all away shows their tiles, not "No papers yet.", and
// the tiles follow the folder (use-papers.ts): a file downloaded, or a folder opened, turns into the papers' tiles.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { mt } from "@clauseo/components/lib/words-matters.ts";
import { rt } from "@clauseo/components/lib/words-room.ts";
import { t } from "@clauseo/components/lib/words.ts";
import { afterEach, expect, test } from "vitest";
import type { MatterSummary } from "../transport/protocol.ts";
import { FakeTransport } from "../transport/fake-transport.ts";
import { uuid } from "../testing/stream-script.ts";
import { detailed, fakeNext } from "./fake-next.ts";
import type { Papers, PaperSummary } from "./host-next.ts";
import { frameBatch } from "../store/batch.ts";
import { StoreContext } from "../store/hooks.ts";
import { createViewStore } from "../store/store.ts";
import { Room } from "./room.tsx";
import { stubFonts } from "./thread/testing.ts";
import { connectRoom } from "./start.ts";
import { loadThread } from "./thread/chunk.ts";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
stubFonts();

let host: HTMLDivElement | null = null;
let root: Root | null = null;

afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  root = null;
  host = null;
});

const MATTER: MatterSummary = {
  matter: uuid(2, 2),
  name: "Sharma v. State of Delhi",
  kind: "folder",
};

const paper = (n: number, path: string): PaperSummary => ({
  file: uuid(n, 8),
  path,
  size: 1000,
  modified: 1_730_000_000_000,
});

type Folder = { papers: PaperSummary[]; rest: Partial<Omit<Papers, "papers">> };

/** A Windows host whose matter's folder is `folder`, synced by OneDrive unless `folder.rest` says otherwise. */
function scripted(folder: Folder, platform: "windows" | "mac" | "linux" = "windows") {
  return new FakeTransport({
    platform,
    query: (query) => {
      if (query === "list_matters") return { matters: [MATTER] };
      if (typeof query === "object" && query.list_threads !== undefined) return { threads: [] };
      throw new Error("not scripted");
    },
    next: fakeNext({
      matters: () => [detailed(MATTER)],
      papers: () => folder.papers,
      listing: () => ({ place: { synced: { service: "one_drive" } }, ...folder.rest }),
    }),
  });
}

const settle = () => act(async () => new Promise((resolve) => setTimeout(resolve, 0)));

async function mount(transport: FakeTransport, language: "en" | "hi" = "en") {
  connectRoom(transport, { language });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () =>
    root?.render(
      <StoreContext value={createViewStore(transport, { batch: () => frameBatch })}>
        <Room transport={transport} />
      </StoreContext>,
    ),
  );
  await settle();
  await act(async () => void (await loadThread()));
}

async function changed(transport: FakeTransport) {
  await act(async () => transport.appEvent({ papers_changed: { matter: MATTER.matter } }));
  await settle();
  await settle();
}

const pane = () => host?.querySelector("aside.room-pane") ?? null;
const tiles = () => [...(pane()?.querySelectorAll("ol.facade > li.tile") ?? [])];
const shown = () =>
  tiles().map((tile) => {
    const name = tile.querySelector(".tile-title")?.textContent ?? "";
    return tile.hasAttribute("data-away")
      ? `${name} (${tile.querySelector(".tile-cue")?.textContent})`
      : name;
  });
const line = () =>
  host?.querySelector('[data-testid="new-conversation-line"]')?.textContent ?? null;
const describedBy = (tile: Element) =>
  document.getElementById(tile.querySelector(".tile-face")?.getAttribute("aria-describedby") ?? "")
    ?.textContent;

const AWAY: Folder = {
  papers: [paper(1, "Plaint.pdf"), paper(2, "Reply.docx")],
  rest: {
    skipped: [
      { path: "Lease deed.pdf", reason: "cloud_only" },
      { path: "Hearing.mp4", reason: "over_size_cap" },
      { path: "Vakalatnama.pdf", reason: "cloud_only" },
    ],
    unread: ["Annexures"],
  },
};

test("files only online and a folder not opened show dimmed among the papers, opening nothing", async () => {
  await mount(scripted(AWAY));
  expect(shown()).toEqual([
    `Annexures (${rt("en", "Not opened")})`,
    `Lease deed.pdf (Only in OneDrive)`,
    "Plaint.pdf",
    "Reply.docx",
    `Vakalatnama.pdf (Only in OneDrive)`,
  ]);
  const away = tiles().filter((tile) => tile.hasAttribute("data-away"));
  for (const tile of away) {
    const face = tile.querySelector(".tile-face");
    expect(face?.getAttribute("aria-disabled")).toBe("true");
    // no Open in Word, no Open it, no Tracked changes
    expect(face?.getAttribute("data-testid")).toBeNull();
    expect(tile.querySelector(".tile-act")).toBeNull();
    expect(tile.id).toMatch(/paper-away-\d$/);
  }
  expect(describedBy(away[0]!)).toBe(
    "Clauseo couldn't look inside this folder. It may be only online, or Clauseo may not be allowed to open it.",
  );
  expect(describedBy(away[1]!)).toBe(
    "Not on this computer, so Clauseo doesn't open it or save new versions of it. " +
      "To keep it here, right-click it in File Explorer and choose Always keep on this device.",
  );
  // the papers' tiles still open
  expect(tiles()[2]?.querySelector(".tile-face")?.getAttribute("aria-disabled")).toBeNull();
  // the count adds the two files only online; the folder isn't counted
  expect(line()).toBe(
    `${mt("en", "A new conversation. It reads this matter's {n} papers.", { n: 2 })} 2 more are only in OneDrive.`,
  );
});

test("on the Mac, in a folder no named service syncs, the tile says online and its tip says download it", async () => {
  await mount(
    scripted(
      {
        papers: [paper(1, "Plaint.pdf")],
        rest: { place: "local", skipped: [{ path: "Lease.pdf", reason: "cloud_only" }] },
      },
      "mac",
    ),
  );
  expect(shown()).toEqual(["Lease.pdf (Only online)", "Plaint.pdf"]);
  expect(describedBy(tiles()[0]!)).toContain("Download it to this computer to change that.");
  expect(line()).toBe(
    `${mt("en", "A new conversation. It reads this matter's one paper.")} 1 more is only online.`,
  );
});

test("a matter whose files are all only online shows their tiles, not No papers yet., and says so in its line", async () => {
  await mount(
    scripted({
      papers: [],
      rest: {
        skipped: [
          { path: "Lease deed.pdf", reason: "cloud_only" },
          { path: "Plaint.pdf", reason: "cloud_only" },
        ],
      },
    }),
  );
  expect(shown()).toEqual(["Lease deed.pdf (Only in OneDrive)", "Plaint.pdf (Only in OneDrive)"]);
  expect(pane()?.textContent).not.toContain(t("en", "No papers yet."));
  expect(line()).toBe("A new conversation. This matter's 2 papers are only in OneDrive.");
});

test("files only online win over folders not opened in the line, which counts the files alone", async () => {
  await mount(
    scripted({
      papers: [],
      rest: {
        skipped: [{ path: "Lease deed.pdf", reason: "cloud_only" }],
        unread: ["Annexures", "Orders"],
      },
    }),
  );
  expect(line()).toBe("A new conversation. This matter's one paper is only in OneDrive.");
});

test("a matter with only a folder not opened shows its tile, not No papers yet.", async () => {
  await mount(scripted({ papers: [], rest: { unread: ["Annexures"] } }));
  expect(shown()).toEqual([`Annexures (${rt("en", "Not opened")})`]);
  expect(pane()?.textContent).not.toContain(t("en", "No papers yet."));
  expect(line()).toBe("A new conversation. Clauseo couldn't open this matter's folder.");
});

test("with two folders not opened and nothing else, the line says folders", async () => {
  await mount(scripted({ papers: [], rest: { place: "local", unread: ["Annexures", "Orders"] } }));
  expect(line()).toBe("A new conversation. Clauseo couldn't open this matter's folders.");
});

test("with only a file over the size cap the wall still says No papers yet.", async () => {
  await mount(
    scripted({ papers: [], rest: { skipped: [{ path: "Hearing.mp4", reason: "over_size_cap" }] } }),
  );
  expect(pane()?.textContent).toBe(t("en", "No papers yet."));
  expect(line()).toBeNull();
});

test("a file downloaded turns into a paper's tile and the count drops; a folder opened shows its papers", async () => {
  const folder: Folder = {
    papers: [paper(1, "Plaint.pdf")],
    rest: {
      skipped: [
        { path: "Lease deed.pdf", reason: "cloud_only" },
        { path: "Vakalatnama.pdf", reason: "cloud_only" },
      ],
      unread: ["Annexures"],
    },
  };
  const transport = scripted(folder);
  await mount(transport);
  expect(line()).toContain("2 more are only in OneDrive.");

  // Lease deed downloaded: a paper now, with an opener
  folder.papers = [paper(2, "Lease deed.pdf"), paper(1, "Plaint.pdf")];
  folder.rest = { ...folder.rest, skipped: [{ path: "Vakalatnama.pdf", reason: "cloud_only" }] };
  await changed(transport);
  expect(shown()).toEqual([
    `Annexures (${rt("en", "Not opened")})`,
    "Lease deed.pdf",
    "Plaint.pdf",
    "Vakalatnama.pdf (Only in OneDrive)",
  ]);
  expect(tiles()[1]?.querySelector(".tile-face")?.getAttribute("data-testid")).toBe("open-file");
  expect(line()).toContain("1 more is only in OneDrive.");

  // the last one downloaded, and the folder opened: its papers in its tile's place, and no more to count
  folder.papers = [
    paper(3, "Annexures/A1.pdf"),
    paper(2, "Lease deed.pdf"),
    paper(1, "Plaint.pdf"),
    paper(4, "Vakalatnama.pdf"),
  ];
  folder.rest = { skipped: [], unread: [] };
  await changed(transport);
  expect(shown()).toEqual(["A1.pdf", "Lease deed.pdf", "Plaint.pdf", "Vakalatnama.pdf"]);
  expect(tiles().some((tile) => tile.hasAttribute("data-away"))).toBe(false);
  expect(line()).toBe(mt("en", "A new conversation. It reads this matter's {n} papers.", { n: 4 }));
});

test("in Hindi the tiles and the count say it in Hindi, the OS's menu names in English", async () => {
  await mount(scripted(AWAY), "hi");
  const away = tiles().filter((tile) => tile.hasAttribute("data-away"));
  expect(away.map((tile) => tile.querySelector(".tile-cue")?.textContent)).toEqual([
    "नहीं खोला गया",
    "केवल OneDrive में",
    "केवल OneDrive में",
  ]);
  expect(describedBy(away[1]!)).toContain("Always keep on this device चुनें।");
  expect(line()).toContain("2 और दस्तावेज़ केवल OneDrive में हैं।");
});

test("in Hindi a matter with nothing to read says so where a new conversation starts", async () => {
  await mount(
    scripted({
      papers: [],
      rest: {
        skipped: [
          { path: "Lease deed.pdf", reason: "cloud_only" },
          { path: "Plaint.pdf", reason: "cloud_only" },
        ],
      },
    }),
    "hi",
  );
  expect(line()).toBe("एक नई बातचीत। इस मामले के 2 दस्तावेज़ केवल OneDrive में हैं।");
  await act(async () => root?.unmount());
  host?.remove();
  await mount(scripted({ papers: [], rest: { unread: ["Annexures"] } }), "hi");
  expect(line()).toBe("एक नई बातचीत। Clauseo इस मामले के फ़ोल्डर नहीं खोल सका।");
});
