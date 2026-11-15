// @vitest-environment happy-dom
// Versions beside the sidebar's toggle on a wide window (1512 px), where the rail has its column: Versions' timeline
// takes the column whether the sidebar is shown or hidden (responsive.css leaves a window with data-panel alone), and
// the toggle, pressed while it stands there, closes Versions and leaves the column as the toggle said.
import { afterEach, beforeEach, expect, test } from "vitest";
import { readLayout, writeLayout } from "../layout-store.ts";
import { mountVersions, version, type VersionsHarness } from "./testing.ts";

let h: VersionsHarness | null = null;
let size = { width: 0, height: 0 };
const viewport = (width: number, height: number) =>
  (window as unknown as { happyDOM: { setViewport: (s: object) => void } }).happyDOM.setViewport({
    width,
    height,
  });

beforeEach(() => {
  size = { width: window.innerWidth, height: window.innerHeight };
  localStorage.clear();
  viewport(1512, 945);
});

afterEach(async () => {
  await h?.unmount();
  h = null;
  localStorage.clear();
  viewport(size.width, size.height);
});

const win = () => h?.host.querySelector<HTMLElement>(".room-window");

test("with the sidebar shown, Versions takes the rail's column, and the toggle closes it and hides the column", async () => {
  h = await mountVersions([version(1, "import")]);
  await h.openVersions();
  expect(h.one("versions")).not.toBeNull();
  expect(win()?.hasAttribute("data-panel")).toBe(true);
  expect(win()?.hasAttribute("data-rail")).toBe(false);
  // the rail isn't in the page while Versions stands there: the toggle names nothing it can't reach
  expect(h.one("rail-toggle")?.hasAttribute("aria-controls")).toBe(false);
  await h.click(h.one("rail-toggle"));
  expect(h.one("versions")).toBeNull();
  expect(win()?.getAttribute("data-rail")).toBe("hidden");
  expect(readLayout("main").sidebar).toBe(false);
});

test("with the sidebar hidden, Versions still takes the column, and the toggle closes it and shows the rail", async () => {
  writeLayout("main", { sidebar: false, papers: true });
  h = await mountVersions([version(1, "import")]);
  expect(win()?.getAttribute("data-rail")).toBe("hidden");
  await h.openVersions();
  expect(h.one("versions")).not.toBeNull();
  // data-panel with data-rail: the hidden sidebar's rules are :not([data-panel]), so the timeline keeps its column
  expect(win()?.hasAttribute("data-panel")).toBe(true);
  await h.click(h.one("rail-toggle"));
  expect(h.one("versions")).toBeNull();
  expect(win()?.hasAttribute("data-rail")).toBe(false);
  expect(h.host.querySelector("nav.room-rail")).not.toBeNull();
  expect(readLayout("main").sidebar).toBe(true);
});
