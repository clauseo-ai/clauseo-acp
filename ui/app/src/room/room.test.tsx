// @vitest-environment happy-dom
// The room's frame over FakeTransport: the title from list_matters, the rail slot Matters opens, the wall with and
// without H8, and the two promises it keeps off its screens: no workspace line, and no "reads them before your first
// question".
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { frame } from "@clauseo/components/lib/words-frame.ts";
import { rt } from "@clauseo/components/lib/words-room.ts";
import { t } from "@clauseo/components/lib/words.ts";
import { vt } from "@clauseo/components/lib/words-versions.ts";
import { afterEach, expect, test, vi } from "vitest";
import type { MatterSummary } from "../transport/protocol.ts";
import { FakeTransport, type FakeHost } from "../transport/fake-transport.ts";
import { uuid } from "../testing/stream-script.ts";
import { detailed, fakeNext } from "./fake-next.ts";
import type { PaperSummary, UiSettings } from "./host-next.ts";
import { Room } from "./room.tsx";
import { connectRoom } from "./start.ts";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement | null = null;
let root: Root | null = null;

const OLDER: MatterSummary = { matter: uuid(1, 2), name: "Acme Ltd v. State", kind: "folder" };
const NEWER: MatterSummary = {
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

/** A fake host with these matters and, where given, H8's papers. */
function fake(matters: MatterSummary[], papers?: PaperSummary[]): FakeTransport {
  const h: FakeHost = {
    query: (query) => {
      if (query === "list_matters") return { matters };
      throw new Error("not scripted");
    },
  };
  if (papers !== undefined)
    h.next = fakeNext({ matters: () => matters.map((m) => detailed(m)), papers });
  return new FakeTransport(h);
}

async function mount(transport: FakeTransport, display: Partial<UiSettings> = {}) {
  connectRoom(transport, display);
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root?.render(<Room transport={transport} />));
  // list_matters, then list_papers
  await act(async () => new Promise((resolve) => setTimeout(resolve, 0)));
}

afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  root = null;
  host = null;
});

const title = () => host?.querySelector('[data-testid="matter"]') ?? null;
const pane = () => host?.querySelector("aside.room-pane") ?? null;

test("the title is the newest matter's name, in English, with nothing under it", async () => {
  await mount(fake([NEWER, OLDER]), { language: "hi" });
  expect(title()?.tagName).toBe("H1");
  expect(title()?.textContent).toBe(NEWER.name);
  expect(title()?.getAttribute("lang")).toBe("en");
  expect(host?.querySelector(".room-next")?.textContent).toBe("");
});

test("with no matters the title says so, in the page's language", async () => {
  await mount(fake([]), { language: "hi" });
  expect(title()?.textContent).toBe(frame("hi", "No matters yet"));
  expect(title()?.getAttribute("lang")).toBe("hi");
});

test("while list_matters is on its way, and after it fails, the toolbar has no heading, never an empty one", async () => {
  const failing = new FakeTransport({
    query: () => {
      throw new Error("the host is gone");
    },
  });
  const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
  try {
    connectRoom(failing, {});
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
    await act(async () => root?.render(<Room transport={failing} />));
    expect(host.querySelector("h1")).toBeNull();
    await act(async () => new Promise((resolve) => setTimeout(resolve, 0)));
    expect(warn).toHaveBeenCalledWith(expect.stringContaining("list_matters failed"));
    expect(host.querySelector("h1")).toBeNull();
    expect(host.querySelector("header.room-top .room-rail-toggle")).not.toBeNull();
  } finally {
    warn.mockRestore();
  }
});

test("the frame has the toolbar, the rail slot, the work and the wall, and no window controls, search or plan", async () => {
  await mount(fake([NEWER]));
  const win = host?.querySelector(".room-window");
  expect(win?.querySelector("header.room-top .room-lead .room-rail-toggle")).not.toBeNull();
  expect(win?.querySelector("main.room-centre")?.childElementCount).toBe(0);
  // the rail (rail.tsx): New matter at its head, the matter's row, the scratch matter at its foot
  expect(win?.querySelector("nav.room-rail [data-testid='new-matter']")).not.toBeNull();
  expect(win?.querySelectorAll("nav.room-rail [data-testid='matter-row']")).toHaveLength(1);
  for (const gone of [
    ".room-lights",
    ".room-caption",
    ".room-search",
    ".room-pair",
    ".room-plan",
    '[role="toolbar"]',
  ]) {
    expect(win?.querySelector(gone), gone).toBeNull();
  }
});

test("Matters controls the rail slot and opens and folds it", async () => {
  await mount(fake([NEWER]));
  const toggle = host?.querySelector<HTMLButtonElement>(".room-rail-toggle");
  const rail = host?.querySelector(".room-rail");
  expect(toggle?.getAttribute("aria-controls")).toBe(rail?.id);
  expect(rail?.id).not.toBe("");
  expect(toggle?.getAttribute("aria-expanded")).toBe("false");
  expect(rail?.hasAttribute("data-open")).toBe(false);
  await act(async () => toggle?.click());
  expect(toggle?.getAttribute("aria-expanded")).toBe("true");
  expect(rail?.hasAttribute("data-open")).toBe(true);
  await act(async () => toggle?.click());
  expect(rail?.hasAttribute("data-open")).toBe(false);
});

test("without H8 the wall shows its surface only, and asks for no papers", async () => {
  const transport = fake([NEWER]);
  await mount(transport);
  expect(pane()?.childElementCount).toBe(0);
  expect(transport.only("next_query")).toEqual([]);
});

test("with H8 the wall has one tile per file, named by its file name, with no page and no chip", async () => {
  const transport = fake(
    [NEWER],
    [paper(1, "papers/Notice to the bank.pdf"), paper(2, "Reply.docx")],
  );
  await mount(transport);
  expect(transport.only("next_query")).toContainEqual({
    call: "next_query",
    query: { list_papers: { matter: NEWER.matter } },
  });
  // a facade of names: its rows keep no room for a page or a chip (wall.css)
  expect(pane()?.querySelector("ol.facade")?.classList.contains("facade-names")).toBe(true);
  const tiles = [...(pane()?.querySelectorAll("ol.facade > li.tile") ?? [])];
  expect(tiles.map((tile) => tile.textContent)).toEqual(["Notice to the bank.pdf", "Reply.docx"]);
  expect(pane()?.querySelector(".tile-no")).toBeNull();
  expect(pane()?.querySelector(".tile-foot")).toBeNull();
});

test("with H8 and no files the wall says there are no papers yet, without the second sentence or a button", async () => {
  await mount(fake([NEWER], []));
  expect(pane()?.textContent).toBe(t("en", "No papers yet."));
  expect(pane()?.querySelector("button")).toBeNull();
});

// HANDOFF §3: the workspace line needs the sandbox (QU.1), and the second sentence the local read.
for (const lang of ["en", "hi"] as const) {
  test(`the room never renders the workspace line or EmptyPapers' second sentence (${lang})`, async () => {
    for (const transport of [fake([NEWER], []), fake([NEWER]), fake([])]) {
      await mount(transport, { language: lang });
      const text = host?.textContent ?? "";
      const workspace = rt(
        lang,
        "Safe workspace. Clauseo works inside this matter's folder and asks before going beyond it.",
      );
      expect(text).not.toContain(workspace.slice(0, 12));
      expect(text).not.toContain(vt(lang, "Not protected"));
      expect(host?.querySelector(".np-mark")).toBeNull();
      expect(text).not.toContain(
        t(lang, "Add the matter's papers and Clauseo reads them before your first question."),
      );
      expect(host?.querySelector(".empty-body")).toBeNull();
      await act(async () => root?.unmount());
      host?.remove();
    }
    root = null;
    host = null;
  });
}
