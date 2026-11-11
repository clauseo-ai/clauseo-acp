// @vitest-environment happy-dom
// Clauseo's lockup at the head of the rail's column: in the toolbar's lead, after the Mac's lights and dragging the
// window as the lead does; at the top of the rail, above New matter; and in macOS's full screen, the lead keeps no room
// for the lights macOS has hidden and starts at the rail's own left, New matter's. Which of the two shows is rail-brand.css's, by width and platform.
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { act, type ReactNode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, test, vi } from "vitest";
import { TooltipProvider } from "../ui/tooltip.tsx";
import { RoomScopeProvider } from "./room-scope-provider.tsx";
import { Rail } from "./rail.tsx";
import { RoomToolbar, type RoomToolbarProps } from "./room-toolbar.tsx";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement | null = null;
let root: Root | null = null;

async function mount(node: ReactNode) {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () =>
    root?.render(
      <TooltipProvider>
        <RoomScopeProvider className="room-window">{node}</RoomScopeProvider>
      </TooltipProvider>,
    ),
  );
}

afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  root = null;
  host = null;
  vi.restoreAllMocks();
});

const toolbar: RoomToolbarProps = {
  title: "Acme Ltd v. State",
  titleHint: "Acme",
  history: [],
  canUndo: false,
  onUndo: () => {},
  railOpen: false,
  onRail: () => {},
  platform: "mac",
  plan: null,
};

/** The screen's height against the window's: a window that fills its screen reads as macOS's full screen. */
const screenTaller = (by: number) =>
  vi.spyOn(window.screen, "height", "get").mockReturnValue(window.innerHeight + by);

test("in the lead the lockup follows the Mac's lights, named Clauseo, and drags the window from its paths", async () => {
  screenTaller(100);
  await mount(<RoomToolbar {...toolbar} nativeLights brand />);
  const lead = host?.querySelector(".room-lead");
  const brand = lead?.querySelector(".room-brand");
  expect(brand?.getAttribute("data-at")).toBe("lead");
  expect(brand?.previousElementSibling?.classList.contains("room-lights")).toBe(true);
  expect(brand?.getAttribute("data-tauri-drag-region")).toBe("deep");
  expect(brand?.querySelector("svg")?.getAttribute("aria-label")).toBe("Clauseo");
  expect(brand?.querySelector("svg")?.getAttribute("role")).toBe("img");
  expect(brand?.querySelector("button, a")).toBeNull();
});

test("without brand the lead holds no lockup", async () => {
  await mount(<RoomToolbar {...toolbar} />);
  expect(host?.querySelector(".room-brand")).toBeNull();
});

test("in macOS's full screen the native lights leave the lead and the lockup takes their place", async () => {
  screenTaller(0);
  await mount(<RoomToolbar {...toolbar} nativeLights brand />);
  expect(host?.querySelector(".room-lights")).toBeNull();
  expect(
    host?.querySelector(".room-lead")?.firstElementChild?.classList.contains("room-brand"),
  ).toBe(true);
  // out of full screen, the window smaller than its screen again: the lights are back
  screenTaller(100);
  await act(async () => window.dispatchEvent(new Event("resize")));
  expect(host?.querySelector(".room-lights")).not.toBeNull();
});

test("drawn lights, as outside the desktop app, stay whatever the window's size", async () => {
  screenTaller(0);
  await mount(<RoomToolbar {...toolbar} brand />);
  expect(host?.querySelector(".room-lights")).not.toBeNull();
});

test("on the rail the lockup tops the head, above New matter, with the platform for its stylesheet; not dragging", async () => {
  await mount(<Rail open groups={[]} brand testIds={{ newMatter: "new-matter" }} />);
  const head = host?.querySelector(".room-rail-head");
  const brand = head?.firstElementChild;
  expect(brand?.classList.contains("room-brand")).toBe(true);
  expect(brand?.getAttribute("data-at")).toBe("rail");
  expect(brand?.getAttribute("data-platform")).toBe("mac");
  expect(brand?.hasAttribute("data-tauri-drag-region")).toBe(false);
  expect(brand?.nextElementSibling?.getAttribute("data-testid")).toBe("new-matter");
});

test("a rail without brand starts with New matter", async () => {
  await mount(<Rail open groups={[]} testIds={{ newMatter: "new-matter" }} />);
  expect(host?.querySelector(".room-brand")).toBeNull();
});

/** The lead's padding in macOS's full screen, from shell.css: the rail's own left, where New matter starts. */
const fullScreenLead = () => {
  const shell = readFileSync(join(dirname(fileURLToPath(import.meta.url)), "shell.css"), "utf8");
  const at = shell.indexOf(".room-top[data-full-screen] .room-lead {");
  return at === -1 ? null : shell.slice(shell.indexOf("{", at) + 1, shell.indexOf("}", at));
};

test("in macOS's full screen the lead drops the lights' room and starts at New matter's left; a window keeps it", async () => {
  // the rail's padding (New matter's left) and the full-screen lead's are the same token
  expect(fullScreenLead()).toMatch(/padding-left: var\(--s3\);/);
  screenTaller(0);
  await mount(<RoomToolbar {...toolbar} nativeLights brand />);
  const top = () => host?.querySelector(".room-top");
  expect(top()?.hasAttribute("data-full-screen")).toBe(true);
  screenTaller(100);
  await act(async () => window.dispatchEvent(new Event("resize")));
  expect(top()?.hasAttribute("data-full-screen")).toBe(false);
  expect(host?.querySelector(".room-lights")?.hasAttribute("data-native")).toBe(true);
});

test("on Windows and Linux a window that fills its screen keeps its lead as it is", async () => {
  screenTaller(0);
  await mount(<RoomToolbar {...toolbar} platform="windows" nativeLights brand />);
  expect(host?.querySelector(".room-top")?.hasAttribute("data-full-screen")).toBe(false);
  await act(async () =>
    root?.render(<RoomToolbar {...toolbar} platform="linux" nativeLights brand />),
  );
  expect(host?.querySelector(".room-top")?.hasAttribute("data-full-screen")).toBe(false);
});

test("the host's word on full screen, where given, stands in for the window's size", async () => {
  screenTaller(100);
  await mount(<RoomToolbar {...toolbar} nativeLights fullScreen brand />);
  expect(host?.querySelector(".room-top")?.hasAttribute("data-full-screen")).toBe(true);
  expect(host?.querySelector(".room-lights")).toBeNull();
  screenTaller(0);
  await act(async () => window.dispatchEvent(new Event("resize")));
  await act(async () =>
    root?.render(<RoomToolbar {...toolbar} nativeLights fullScreen={false} brand />),
  );
  expect(host?.querySelector(".room-top")?.hasAttribute("data-full-screen")).toBe(false);
  expect(host?.querySelector(".room-lights")).not.toBeNull();
});
