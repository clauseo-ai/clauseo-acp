// @vitest-environment happy-dom
// The room's frame over FakeTransport: the title from list_matters, the rail slot Matters opens, the wall with and
// without H8, and the two promises it keeps off its screens: no workspace line, and no "reads them before your first
// question".
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { frame } from "@clauseo/components/lib/words-frame.ts";
import { mt } from "@clauseo/components/lib/words-matters.ts";
import { rt } from "@clauseo/components/lib/words-room.ts";
import { t } from "@clauseo/components/lib/words.ts";
import { vt } from "@clauseo/components/lib/words-versions.ts";
import { afterEach, expect, test, vi } from "vitest";
import type { MatterSummary } from "../transport/protocol.ts";
import type { WindowControls } from "../transport/window-controls.ts";
import { FakeTransport, type FakeHost } from "../transport/fake-transport.ts";
import { uuid } from "../testing/stream-script.ts";
import { detailed, fakeNext } from "./fake-next.ts";
import type { MatterInfo, PaperSummary, UiSettings } from "./host-next.ts";
import { frameBatch } from "../store/batch.ts";
import { StoreContext } from "../store/hooks.ts";
import { createViewStore } from "../store/store.ts";
import { Room } from "./room.tsx";
import { stubFonts } from "./thread/testing.ts";
import { connectRoom } from "./start.ts";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
stubFonts();

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

/** A fake host with these matters and, where given, H8's papers; on Linux unless `platform` says. */
function fake(
  matters: MatterSummary[],
  papers?: PaperSummary[],
  platform?: FakeHost["platform"],
): FakeTransport {
  const h: FakeHost = {
    ...(platform === undefined ? {} : { platform }),
    query: (query) => {
      if (query === "list_matters") return { matters };
      if (typeof query === "object" && query.list_threads !== undefined) return { threads: [] };
      throw new Error("not scripted");
    },
  };
  if (papers !== undefined)
    h.next = fakeNext({ matters: () => matters.map((m) => detailed(m)), papers });
  return new FakeTransport(h);
}

/** The room as App mounts it: inside the page's view store. */
const inStore = (transport: FakeTransport, windowControls?: WindowControls) => (
  <StoreContext value={createViewStore(transport, { batch: () => frameBatch })}>
    <Room transport={transport} windowControls={windowControls} />
  </StoreContext>
);

async function mount(
  transport: FakeTransport,
  display: Partial<UiSettings> = {},
  windowControls?: WindowControls,
) {
  connectRoom(transport, display);
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root?.render(inStore(transport, windowControls)));
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
    await act(async () => root?.render(inStore(failing)));
    expect(host.querySelector("h1")).toBeNull();
    // the rail while the list is on its way: no "No matters yet", no scratch row that opens nothing
    const rail = () => host?.querySelector("nav.room-rail");
    expect(rail()?.textContent).not.toContain("No matters yet");
    expect(rail()?.querySelector("[data-kind=scratch]")).toBeNull();
    await act(async () => new Promise((resolve) => setTimeout(resolve, 0)));
    expect(warn).toHaveBeenCalledWith(expect.stringContaining("list_matters failed"));
    expect(host.querySelector("h1")).toBeNull();
    expect(host.querySelector("header.room-top .room-rail-toggle")).not.toBeNull();
    // and after it failed: still neither, nor a Find field with nothing to find
    expect(rail()?.textContent).not.toContain("No matters yet");
    expect(rail()?.querySelector("[data-kind=scratch]")).toBeNull();
    expect(rail()?.querySelector("[data-testid=find]")).toBeNull();
  } finally {
    warn.mockRestore();
  }
});

test("the frame has the toolbar, the rail slot, the work and the wall, and no search or plan", async () => {
  await mount(fake([NEWER]));
  const win = host?.querySelector(".room-window");
  expect(win?.querySelector("header.room-top .room-lead .room-rail-toggle")).not.toBeNull();
  // the work: the thread's list and the composer (thread/work.test.tsx tests them)
  const centre = win?.querySelector("main.room-centre");
  expect(centre?.querySelector('[data-testid="transcript"] .room-thread')).not.toBeNull();
  expect(centre?.querySelector('form.room-composer [data-testid="composer"]')).not.toBeNull();
  // the rail (rail.tsx): New matter at its head, the matter's row, the scratch matter at its foot
  expect(win?.querySelector("nav.room-rail [data-testid='new-matter']")).not.toBeNull();
  expect(win?.querySelectorAll("nav.room-rail [data-testid='matter-row']")).toHaveLength(1);
  for (const gone of [".room-search", ".room-pair", ".room-plan", '[role="toolbar"]']) {
    expect(win?.querySelector(gone), gone).toBeNull();
  }
});

test("the window draws its own frame: caption buttons off the Mac, painted lights on it, outside the desktop app", async () => {
  const info = vi.spyOn(console, "info").mockImplementation(() => {});
  try {
    await mount(fake([NEWER], undefined, "windows"));
    expect(host?.querySelector(".room-lights")).toBeNull();
    const max = host?.querySelector<HTMLButtonElement>(
      'header.room-top .room-caption [data-caption="maximise"]',
    );
    expect(max).not.toBeNull();
    // no desktop window to call: the button's own fallback
    await act(async () => max?.click());
    expect(info).toHaveBeenCalledWith("Clauseo would maximise the window.");
    await act(async () => root?.unmount());
    host?.remove();
    await mount(fake([NEWER], undefined, "mac"));
    const lights = host?.querySelector("header.room-top .room-lead .room-lights");
    expect(lights).not.toBeNull();
    expect(lights?.hasAttribute("data-native")).toBe(false);
    expect(host?.querySelector(".room-caption")).toBeNull();
  } finally {
    info.mockRestore();
  }
});

test("the room keeps the toolbar's height on the window as --top-h", async () => {
  const seen: ResizeObserverCallback[] = [];
  vi.stubGlobal(
    "ResizeObserver",
    class {
      constructor(cb: ResizeObserverCallback) {
        seen.push(cb);
      }
      observe() {}
      disconnect() {}
    },
  );
  try {
    await mount(fake([NEWER]));
    const top = host?.querySelector<HTMLElement>("header.room-top");
    const win = host?.querySelector<HTMLElement>(".room-window");
    if (top === null || top === undefined) throw new Error("no toolbar");
    Object.defineProperty(top, "offsetHeight", { configurable: true, value: 112 });
    for (const cb of seen) cb([], {} as ResizeObserver);
    // on the room's window node (the scope's own element, around .room-window), so every part inherits it
    const holder = host?.querySelector<HTMLElement>('[style*="--top-h"]');
    expect(holder?.style.getPropertyValue("--top-h")).toBe("112px");
    expect(win !== null && win !== undefined && holder?.contains(win)).toBe(true);
  } finally {
    vi.unstubAllGlobals();
  }
});

test("connecting the room marks <html> for the frame contract", async () => {
  delete document.documentElement.dataset.frame;
  await mount(fake([NEWER]));
  expect(document.documentElement.dataset.frame).toBe("own");
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

/** A NextHost with these matters, details and all. */
const withDetails = (matters: MatterInfo[]) =>
  new FakeTransport({ next: fakeNext({ matters: () => matters }) });

test("Matters carries the court-yellow dot while another matter waits with a question or for permission, never for the limit", async () => {
  const dot = () =>
    host?.querySelector("[data-testid=rail-toggle]")?.classList.contains("room-rail-waits");
  const m = (n: number, status: MatterInfo["status"], over: Partial<MatterInfo> = {}) =>
    detailed({ matter: uuid(n, 2), name: `Matter ${n}`, kind: "folder" }, { status, ...over });
  for (const [status, waits] of [
    ["needs_answer", true],
    ["needs_permission", true],
    [{ paused_at_limit: {} }, false],
    ["working", false],
  ] as const) {
    await mount(withDetails([m(1, "idle", { last_opened: 10 }), m(2, status)]));
    expect(dot(), JSON.stringify(status)).toBe(waits);
    await act(async () => root?.unmount());
    host?.remove();
  }
  // the open matter's own question is in view: no dot for it
  await mount(withDetails([m(1, "needs_answer", { last_opened: 10 }), m(2, "idle")]));
  expect(dot()).toBe(false);
});

test("the open scratch matter says its line under its title, in the design's words", async () => {
  const scratch: MatterSummary = { matter: uuid(9, 2), name: "Scratch", kind: "scratch" };
  for (const language of ["en", "hi"] as const) {
    await mount(fake([NEWER, scratch]), { language });
    expect(host?.querySelector(".room-next")?.textContent).toBe("");
    await act(async () => host?.querySelector<HTMLElement>("[data-kind=scratch]")?.click());
    expect(host?.querySelector("header.room-top")?.textContent).toContain(
      mt(language, "Quick questions, kept like any matter"),
    );
    await act(async () => root?.unmount());
    host?.remove();
  }
});

test("H8 before H5: with only the papers part the wall has its tiles and the rail today's list, with no details", async () => {
  const transport = new FakeTransport({
    query: (query) => {
      if (query === "list_matters") return { matters: [NEWER, OLDER] };
      throw new Error("not scripted");
    },
    next: {
      ...fakeNext({ matters: () => [], papers: [paper(1, "Reply.docx")] }),
      parts: ["papers"],
    },
  });
  await mount(transport);
  expect(title()?.textContent).toBe(NEWER.name);
  expect(host?.querySelectorAll("[data-testid=matter-row]")).toHaveLength(2);
  expect([...(pane()?.querySelectorAll("li.tile") ?? [])].map((t) => t.textContent)).toEqual([
    "Reply.docx",
  ]);
  expect(transport.only("next_query").map((q) => q.query)).toEqual([
    { list_papers: { matter: NEWER.matter } },
  ]);
  // find names only: no client to search without H5
  expect(host?.querySelector("[data-testid=find]")?.closest("label")?.textContent).toContain(
    mt("en", "Find a matter by its name"),
  );
});

test("the keyboard at the folded width: a row opened folds the rail and focus goes back to Matters; Escape does too", async () => {
  await mount(fake([NEWER, OLDER]));
  const toggle = () => host?.querySelector<HTMLButtonElement>("[data-testid=rail-toggle]");
  const rail = () => host?.querySelector("nav.room-rail");
  const row = (name: string) =>
    [...(rail()?.querySelectorAll<HTMLButtonElement>("[data-testid=matter-row]") ?? [])].find((r) =>
      r.textContent?.includes(name),
    );
  await act(async () => toggle()?.click());
  expect(rail()?.hasAttribute("data-open")).toBe(true);
  // focus on the row, then Enter: happy-dom has no Tab order or key activation, so the row is focused and clicked as
  // Enter clicks a focused button
  await act(async () => row(OLDER.name)?.focus());
  expect(document.activeElement).toBe(row(OLDER.name));
  await act(async () => {
    document.activeElement?.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Enter", bubbles: true }),
    );
    (document.activeElement as HTMLElement | null)?.click();
  });
  expect(title()?.textContent).toBe(OLDER.name);
  expect(rail()?.hasAttribute("data-open")).toBe(false);
  expect(document.activeElement).toBe(toggle());
  // opened again, Escape on a row folds it and focus goes back to Matters
  await act(async () => toggle()?.click());
  await act(async () => row(NEWER.name)?.focus());
  await act(async () =>
    document.activeElement?.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Escape", bubbles: true }),
    ),
  );
  expect(rail()?.hasAttribute("data-open")).toBe(false);
  expect(document.activeElement).toBe(toggle());
  // Escape in Find with words clears them and keeps focus there, with the rail open; on the empty field it folds
  await act(async () => toggle()?.click());
  const find = host?.querySelector<HTMLInputElement>("[data-testid=find]");
  await act(async () => find?.focus());
  await act(async () => {
    Reflect.set(HTMLInputElement.prototype, "value", "acme", find);
    find?.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await act(async () =>
    find?.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true })),
  );
  expect(find?.value).toBe("");
  expect(document.activeElement).toBe(find);
  expect(rail()?.hasAttribute("data-open")).toBe(true);
  await act(async () =>
    find?.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true })),
  );
  expect(rail()?.hasAttribute("data-open")).toBe(false);
  expect(document.activeElement).toBe(toggle());
});

/** Window controls that count their calls. */
const countingControls = () =>
  ({
    minimise: vi.fn(async () => {}),
    toggleMaximise: vi.fn(async () => {}),
    close: vi.fn(async () => {}),
    isMaximised: vi.fn(async () => false),
  }) satisfies WindowControls;

/** Two resizes, each with its animation frame run. */
async function resizeTwice() {
  for (let i = 0; i < 2; i++) {
    await act(async () => window.dispatchEvent(new Event("resize")));
    await act(async () => new Promise((resolve) => setTimeout(resolve, 20)));
  }
}

test("on the Mac the room asks the window nothing: its own lights need no window commands", async () => {
  vi.stubGlobal("navigator", { platform: "MacIntel", userAgent: "Macintosh" });
  try {
    const controls = countingControls();
    await mount(fake([NEWER], undefined, "mac"), {}, controls);
    await resizeTwice();
    expect(controls.isMaximised).not.toHaveBeenCalled();
    expect(host?.querySelector(".room-lights[data-native]")).not.toBeNull();
    expect(host?.querySelector(".room-caption")).toBeNull();
  } finally {
    vi.unstubAllGlobals();
  }
});

test("on Windows the room's caption buttons follow the window and reach it", async () => {
  const controls = countingControls();
  await mount(fake([NEWER], undefined, "windows"), {}, controls);
  await resizeTwice();
  expect(controls.isMaximised).toHaveBeenCalled();
  const minimise = host?.querySelector<HTMLButtonElement>(".room-caption button");
  await act(async () => minimise?.click());
  expect(controls.minimise).toHaveBeenCalledTimes(1);
});
