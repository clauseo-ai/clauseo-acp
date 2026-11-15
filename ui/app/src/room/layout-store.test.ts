// @vitest-environment happy-dom
// The room's layout per window (layout-store.ts): both shown until set, kept as two flags under
// clauseo.layout.<window label>, each window its own, and a missing or unreadable value read as both shown.
import { afterEach, expect, test, vi } from "vitest";
import { readLayout, SHOWN, writeLayout } from "./layout-store.ts";
import { windowLabel } from "../transport/window-label.ts";

afterEach(() => {
  localStorage.clear();
  vi.restoreAllMocks();
});

test("a window never set shows both", () => {
  expect(readLayout("main")).toEqual({ sidebar: true, papers: true });
  expect(SHOWN).toEqual({ sidebar: true, papers: true });
});

test("what is written is read back, as two flags under clauseo.layout.<label>", () => {
  writeLayout("main", { sidebar: false, papers: true });
  expect(readLayout("main")).toEqual({ sidebar: false, papers: true });
  expect(JSON.parse(localStorage.getItem("clauseo.layout.main") ?? "null")).toEqual({
    sidebar: false,
    papers: true,
  });
  writeLayout("main", { sidebar: true, papers: false });
  expect(readLayout("main")).toEqual({ sidebar: true, papers: false });
});

test("a corrupt or partial value reads as both shown", () => {
  for (const raw of ["{", "null", "42", '"x"', '{"sidebar":false}', '{"sidebar":0,"papers":1}']) {
    localStorage.setItem("clauseo.layout.main", raw);
    expect(readLayout("main"), raw).toEqual(SHOWN);
  }
});

test("each window keeps its own", () => {
  writeLayout("main", { sidebar: false, papers: false });
  expect(readLayout("settings")).toEqual(SHOWN);
  writeLayout("settings", { sidebar: true, papers: false });
  expect(readLayout("main")).toEqual({ sidebar: false, papers: false });
  expect(readLayout("settings")).toEqual({ sidebar: true, papers: false });
});

test("with storage off, the layout holds for this run", () => {
  vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
    throw new Error("off");
  });
  vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
    throw new Error("off");
  });
  writeLayout("run", { sidebar: false, papers: true });
  expect(readLayout("run")).toEqual({ sidebar: false, papers: true });
});

test("outside Tauri the window is main", () => {
  expect(windowLabel()).toBe("main");
});
