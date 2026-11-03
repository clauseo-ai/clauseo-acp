// @vitest-environment happy-dom
import { act, type ReactNode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, test } from "vitest";
import { ToolGlyph } from "./tool-glyph.tsx";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement | null = null;
let root: Root | null = null;

async function mount(node: ReactNode) {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root?.render(node));
  return host;
}

afterEach(async () => {
  await act(async () => root?.unmount());
  document.body.replaceChildren();
  root = null;
  host = null;
});

const svg = (el: HTMLElement) => el.querySelector("svg")!;

test("on draws the glyph alone: no slash, no lock, no mask", async () => {
  for (const glyph of ["web", "browser"] as const) {
    const el = await mount(<ToolGlyph glyph={glyph} state="on" />);
    expect(el.querySelector("[data-part=slash]")).toBeNull();
    expect(el.querySelector("[data-part=lock]")).toBeNull();
    expect(el.querySelector("mask")).toBeNull();
    expect(el.querySelector("svg svg")).not.toBeNull();
    await act(async () => root?.unmount());
  }
});

test("off dims the glyph under a slash, the shape knocked out either side of it", async () => {
  const el = await mount(<ToolGlyph glyph="web" state="off" size={16} />);
  const slash = el.querySelector("[data-part=slash]");
  expect(slash?.getAttribute("d")).toBe("M2.75 2.75l14.5 14.5");
  const mask = el.querySelector("mask");
  expect(mask).not.toBeNull();
  const knockout = mask!.querySelector("[data-part=knockout]");
  expect(knockout?.getAttribute("d")).toBe(slash?.getAttribute("d"));
  // the 1.5 px stroke and a 1.5 px gap either side
  expect(knockout?.getAttribute("stroke-width")).toBe("4.5");
  const masked = el.querySelector("g[mask]");
  expect(masked?.getAttribute("mask")).toBe(`url(#${mask!.id})`);
  expect(masked?.classList.contains("text-muted")).toBe(true);
  expect(el.querySelector("[data-part=lock]")).toBeNull();
  expect(svg(el).getAttribute("width")).toBe("16");
});

test("locked dims the glyph and gives its lower right corner to a lock, knocked out under it", async () => {
  const el = await mount(<ToolGlyph glyph="browser" state="locked" size={11} />);
  // big enough to read at 14 px: the lock (about 16.25 units drawn) scaled to 11 to 12 of the 20, at the lower right
  const at = /translate\(([\d.]+) ([\d.]+)\) scale\(([\d.]+)\)/.exec(
    el.querySelector("[data-part=lock]")?.getAttribute("transform") ?? "",
  );
  const [x, y, k] = [Number(at?.[1]), Number(at?.[2]), Number(at?.[3])];
  expect(16.25 * k).toBeGreaterThanOrEqual(11);
  expect(16.25 * k).toBeLessThanOrEqual(12);
  // its body's right and bottom edges inside the grid, near its lower right corner
  expect(17.25 * k + x).toBeGreaterThan(18);
  expect(17.25 * k + x + 0.75).toBeLessThanOrEqual(20);
  expect(18.25 * k + y).toBeGreaterThan(18);
  expect(18.25 * k + y + 0.75).toBeLessThanOrEqual(20);
  expect(el.querySelector("mask [data-part=knockout]")?.getAttribute("transform")).toBe(
    el.querySelector("[data-part=lock]")?.getAttribute("transform"),
  );
  expect(el.querySelector("[data-part=lock]")?.querySelectorAll("path").length).toBe(2);
  expect(el.querySelector("mask [data-part=knockout]")).not.toBeNull();
  expect(el.querySelector("g[mask]")?.classList.contains("text-muted")).toBe(true);
  expect(el.querySelector("[data-part=slash]")).toBeNull();
});

test("hidden from assistive technology unless labelled, then named as an image", async () => {
  const hidden = await mount(<ToolGlyph glyph="web" state="off" />);
  expect(svg(hidden).getAttribute("aria-hidden")).toBe("true");
  expect(svg(hidden).getAttribute("role")).toBeNull();
  await act(async () => root?.unmount());
  const named = await mount(<ToolGlyph glyph="web" state="off" label="Web search: off" />);
  expect(svg(named).getAttribute("role")).toBe("img");
  expect(svg(named).getAttribute("aria-label")).toBe("Web search: off");
});

test("two glyphs on a page keep their own masks", async () => {
  const el = await mount(
    <>
      <ToolGlyph glyph="web" state="off" />
      <ToolGlyph glyph="browser" state="locked" />
    </>,
  );
  const ids = [...el.querySelectorAll("mask")].map((m) => m.id);
  expect(new Set(ids).size).toBe(2);
});
