// @vitest-environment happy-dom
// The wall follows the open matter's folder (decision record 0055): the host's papers_changed, and the window coming back
// to the front, read the papers again, so a paper added, renamed or taken out shows as it is (D-055, D-056). A folder
// that goes while the matter is open turns the work column into the moved folder's line, with Locate folder on the
// line itself, and empties the wall; Locate folder brings the papers back (D-057).
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { mt } from "@clauseo/components/lib/words-matters.ts";
import { ui } from "@clauseo/components/lib/words-ui.ts";
import { t } from "@clauseo/components/lib/words.ts";
import { afterEach, expect, test } from "vitest";
import type { MatterSummary } from "../transport/protocol.ts";
import { FakeTransport } from "../transport/fake-transport.ts";
import { uuid } from "../testing/stream-script.ts";
import { detailed, fakeNext } from "./fake-next.ts";
import type { MatterInfo, NextCommand, PaperSummary } from "./host-next.ts";
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
const OTHER = uuid(3, 2);

const paper = (n: number, path: string): PaperSummary => ({
  file: uuid(n, 8),
  path,
  size: 1000,
  modified: 1_730_000_000_000,
});

/** A host whose folder is `folder.papers`, or gone while `folder.gone`, and whose matter row is `row.info`. */
function scripted() {
  const folder: { papers: PaperSummary[]; gone: boolean } = { papers: [], gone: false };
  const row = { info: detailed(MATTER) };
  const sent: NextCommand[] = [];
  const transport = new FakeTransport({
    pickFolder: () => ({ folder: "ref-found", display: "Desktop/Sharma" }),
    query: (query) => {
      if (query === "list_matters") return { matters: [MATTER] };
      if (typeof query === "object" && query.list_threads !== undefined) return { threads: [] };
      throw new Error("not scripted");
    },
    next: fakeNext({
      matters: () => [row.info],
      papers: () => {
        if (folder.gone) throw { code: "matter.folder_missing", message: "gone", retryable: true };
        return folder.papers;
      },
      command: (c) => void sent.push(c),
    }),
  });
  return { transport, folder, row, sent };
}

const settle = () => act(async () => new Promise((resolve) => setTimeout(resolve, 0)));

async function mount(transport: FakeTransport) {
  connectRoom(transport, {});
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

const pane = () => host?.querySelector("aside.room-pane") ?? null;
const tiles = () =>
  [...(pane()?.querySelectorAll("ol.facade > li.tile .tile-title") ?? [])].map(
    (n) => n.textContent,
  );
const lists = (transport: FakeTransport) =>
  transport
    .only("next_query")
    .filter((c) => typeof c.query === "object" && "list_papers" in c.query).length;

/** The host's app event, then a frame for the matters' batch and the read it starts. */
async function hear(transport: FakeTransport, event: Parameters<FakeTransport["appEvent"]>[0]) {
  await act(async () => transport.appEvent(event));
  await settle();
  await settle();
}

test("a paper added, renamed or taken out while the matter is open shows on the wall at the host's word", async () => {
  const { transport, folder } = scripted();
  await mount(transport);
  expect(pane()?.textContent).toBe(t("en", "No papers yet."));

  // added in Finder (D-056)
  folder.papers = [paper(1, "Notice to the bank.pdf")];
  await hear(transport, { papers_changed: { matter: MATTER.matter } });
  expect(tiles()).toEqual(["Notice to the bank.pdf"]);

  // renamed, and one more added: the old name's tile goes (D-055)
  folder.papers = [paper(2, "Notice (final).pdf"), paper(3, "Reply.docx")];
  await hear(transport, { papers_changed: { matter: MATTER.matter } });
  expect(tiles()).toEqual(["Notice (final).pdf", "Reply.docx"]);

  // moved out of the folder: one tile left, and the new conversation's count says so
  folder.papers = [paper(3, "Reply.docx")];
  await hear(transport, { papers_changed: { matter: MATTER.matter } });
  expect(tiles()).toEqual(["Reply.docx"]);
  expect(host?.querySelector('[data-testid="new-conversation-line"]')?.textContent).toBe(
    mt("en", "A new conversation. It reads this matter's one paper."),
  );
});

test("another matter's change reads nothing; the window coming back to the front reads the papers again", async () => {
  const { transport, folder } = scripted();
  await mount(transport);
  const before = lists(transport);
  await hear(transport, { papers_changed: { matter: OTHER } });
  expect(lists(transport)).toBe(before);

  // a change the host couldn't watch shows once the lawyer comes back to the window
  folder.papers = [paper(4, "Vakalatnama.pdf")];
  await act(async () => window.dispatchEvent(new Event("focus")));
  await settle();
  expect(lists(transport)).toBe(before + 1);
  expect(tiles()).toEqual(["Vakalatnama.pdf"]);
});

test("a folder that goes while the matter is open shows Locate folder on its line, and Locate brings the papers back", async () => {
  const { transport, folder, row, sent } = scripted();
  folder.papers = [paper(1, "Brief.docx")];
  await mount(transport);
  expect(tiles()).toEqual(["Brief.docx"]);
  expect(host?.textContent).not.toContain(mt("en", "This matter's folder has moved."));

  // deleted in Finder: the host's watch says so, and the row reads missing (D-057)
  folder.gone = true;
  row.info = detailed(MATTER, { folder_state: "missing" });
  await hear(transport, { papers_changed: { matter: MATTER.matter } });
  await hear(transport, { matters_changed: { matters: [row.info], removed: [], all: false } });
  const line = host?.querySelector(".room-moved");
  expect(line?.getAttribute("role")).toBe("status");
  expect(line?.textContent).toContain(mt("en", "This matter's folder has moved."));
  const locate = line?.querySelector("button");
  expect(locate?.textContent).toBe(ui("en", "Locate folder"));
  // the wall no longer shows the papers of a folder that isn't there
  expect(tiles()).toEqual([]);

  // Locate folder, on the line itself: the picker, then relocate_matter; the host then says the folder is back
  await act(async () => locate?.click());
  await settle();
  expect(transport.only("pick_folder")).toHaveLength(1);
  expect(sent).toContainEqual({ relocate_matter: { matter: MATTER.matter, folder: "ref-found" } });
  folder.gone = false;
  row.info = detailed(MATTER) satisfies MatterInfo;
  await hear(transport, { matters_changed: { matters: [row.info], removed: [], all: false } });
  await hear(transport, { papers_changed: { matter: MATTER.matter } });
  expect(host?.querySelector(".room-moved")).toBeNull();
  expect(tiles()).toEqual(["Brief.docx"]);
});
