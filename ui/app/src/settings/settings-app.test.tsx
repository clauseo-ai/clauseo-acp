// @vitest-environment happy-dom
// The settings window over FakeTransport: the sections built so far and no later one, each choice one patch to the
// display store and the display it moves <html> to, without a host written to localStorage["clauseo.display"], with
// one sent to the host as set_ui_settings and followed through settings_changed, a locked key's row disabled with its
// note, and the window's caption buttons off the Mac. axe-core over every section, in both languages and lights.
import axe from "axe-core";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { settled } from "@clauseo/components/lib/late.ts";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { connectDisplay } from "../display/connect.ts";
import type { DisplayStore } from "../room/display-store.ts";
import { DISPLAY_KEY } from "../room/display-store.ts";
import type { Settings } from "../room/host-next.ts";
import { stubFonts } from "../room/thread/testing.ts";
import { FakeTransport } from "../transport/fake-transport.ts";
import type { WindowControls } from "../transport/window-controls.ts";
import { SettingsApp } from "./settings-app.tsx";

vi.mock("./version.ts", () => ({ APP_VERSION: "9.8.7" }));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
stubFonts();

let host: HTMLDivElement | null = null;
let root: Root | null = null;
let store: DisplayStore | null = null;

/** The page's localStorage: Node's own global shadows happy-dom's, so the test brings one. */
const items = new Map<string, string>();
vi.stubGlobal("localStorage", {
  getItem: (key: string) => items.get(key) ?? null,
  setItem: (key: string, value: string) => void items.set(key, value),
  removeItem: (key: string) => void items.delete(key),
  clear: () => items.clear(),
});

beforeEach(() => {
  localStorage.clear();
  history.replaceState(null, "", "/settings.html");
});

afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  store?.dispose();
  root = null;
  host = null;
  store = null;
});

const HOST: Settings = {
  ui: { theme: "system", font_size: 16, language: "en", lite: "automatic", motion: "system" },
  locked: [],
};

/** A transport whose host has H7's settings, answering get_settings with `settings`. */
const withHost = (settings: Settings = HOST) =>
  new FakeTransport({
    next: {
      query: (query) => {
        if (query === "get_settings") return { settings };
        throw new Error("not scripted");
      },
    },
  });

async function mount(
  transport = new FakeTransport(),
  windowControls: WindowControls | null = null,
): Promise<{ transport: FakeTransport; store: DisplayStore; el: HTMLDivElement }> {
  const made = connectDisplay(transport).store;
  store = made;
  const el = document.createElement("div");
  host = el;
  document.body.append(el);
  root = createRoot(el);
  await act(async () =>
    root?.render(
      <SettingsApp store={made} transport={transport} windowControls={windowControls} />,
    ),
  );
  await act(async () => settled());
  return { transport, store: made, el };
}

const html = () => document.documentElement;
const navNames = (el: HTMLElement) =>
  [...el.querySelectorAll('nav[aria-label="Sections"] .set-nav-name')].map((n) => n.textContent);
const press = async (target: Element | null | undefined) => {
  expect(target).toBeTruthy();
  await act(async () => (target as HTMLElement).click());
};
const section = (el: HTMLElement, name: string) =>
  press(
    [...el.querySelectorAll<HTMLButtonElement>(".set-nav-row")].find(
      (b) => b.querySelector(".set-nav-name")?.textContent === name,
    ),
  );
/** The side of the segmented switch named `group` that reads `label`. */
const side = (el: HTMLElement, group: string, label: string) =>
  [...el.querySelectorAll<HTMLButtonElement>(`[aria-label="${group}"] button`)].find(
    (b) => b.textContent === label,
  );
const saved = () => JSON.parse(localStorage.getItem(DISPLAY_KEY) ?? "null") as unknown;

test("the sections are Language, Appearance and About, in the design's order; no later section renders", async () => {
  const { el } = await mount();
  expect(navNames(el)).toEqual(["Language", "Appearance", "About"]);
  for (const later of [
    "Plans",
    "Plan and usage",
    "Indian Kanoon",
    "Matters",
    "Safe workspace",
    "Updates",
    "Privacy",
  ])
    expect(el.textContent).not.toContain(later);
  // the first opens, as the pane's h2, under the window's one h1
  expect(el.querySelector('[aria-current="page"] .set-nav-name')?.textContent).toBe("Language");
  expect(el.querySelector(".set-pane h2")?.textContent).toBe("Language");
  expect(el.querySelectorAll("h1")).toHaveLength(1);
  expect(document.title).toBe("Settings");
  expect(el.querySelector(".set-pane-lead")?.textContent).toBe(
    "The language of every word on screen. Papers, quotes and case names stay as filed.",
  );
});

test("the address's hash opens its section, and a section pressed goes into the address", async () => {
  history.replaceState(null, "", "/settings.html#appearance");
  const { el } = await mount();
  expect(el.querySelector(".set-pane h2")?.textContent).toBe("Appearance");
  await section(el, "About");
  expect(location.hash).toBe("#about");
  expect(el.querySelector(".set-pane h2")?.textContent).toBe("About");
});

test("an unknown or later section's hash opens the first", async () => {
  history.replaceState(null, "", "/settings.html#updates");
  const { el } = await mount();
  expect(el.querySelector(".set-pane h2")?.textContent).toBe("Language");
});

test("without a host each choice moves <html> and writes clauseo.display, one key at a time", async () => {
  history.replaceState(null, "", "/settings.html#appearance");
  const { el } = await mount();
  await press(side(el, "Theme", "Night"));
  expect(html().dataset["theme"]).toBe("night");
  expect(saved()).toMatchObject({ theme: "dark" });
  await press(side(el, "Theme", "Day"));
  expect(html().dataset["theme"]).toBe("day");
  expect(saved()).toMatchObject({ theme: "light" });

  await press(side(el, "Reduced motion", "On"));
  expect(html().dataset["motion"]).toBe("reduced");
  expect(saved()).toMatchObject({ motion: "reduced" });

  const radio = (label: string) =>
    [...el.querySelectorAll('[role="radio"]')].find((r) =>
      r.closest(".flex.items-start")?.textContent?.startsWith(label),
    );
  await press(radio("On"));
  expect(html().dataset["render"]).toBe("lite");
  expect(saved()).toMatchObject({ lite: "lite" });
  await press(radio("Off"));
  expect(html().dataset["render"]).toBe("full");
  expect(saved()).toMatchObject({ lite: "full" });

  // the side's note follows the display
  expect(el.textContent).toContain("Day, text size 16");

  await section(el, "Language");
  await press(side(el, "Language", "हिन्दी"));
  expect(html().lang).toBe("hi");
  expect(saved()).toMatchObject({ language: "hi" });
  // the window's own words follow at once
  expect(el.querySelector(".set-pane h2")?.textContent).toBe("भाषा");
  expect(el.querySelector('nav[aria-label="अनुभाग"]')).not.toBeNull();
});

test("text size offers 14 to 20, shows 16 by default, and a choice sets data-text-size", async () => {
  history.replaceState(null, "", "/settings.html#appearance");
  const { el } = await mount();
  expect(html().dataset["textSize"]).toBe("16");
  const trigger = el.querySelector<HTMLElement>('[role="combobox"]');
  expect(trigger?.textContent).toContain("16");
  await press(trigger);
  await act(async () => settled());
  const options = [...document.querySelectorAll<HTMLElement>('[role="option"]')];
  expect(options.map((o) => o.textContent)).toEqual(["14", "15", "16", "17", "18", "19", "20"]);
  await press(options.find((o) => o.textContent === "18"));
  expect(html().dataset["textSize"]).toBe("18");
  expect(saved()).toMatchObject({ font_size: 18 });
});

test("lite offers On and Off only, and a stored automatic shows as Off", async () => {
  localStorage.setItem(DISPLAY_KEY, JSON.stringify({ lite: "automatic" }));
  history.replaceState(null, "", "/settings.html#appearance");
  const { el } = await mount();
  const radios = [...el.querySelectorAll('[role="radio"]')];
  expect(radios).toHaveLength(2);
  const fieldset = radios[0]?.closest('[role="radiogroup"]');
  expect(fieldset?.textContent).toContain("Lite mode");
  expect(fieldset?.textContent).not.toContain("Automatic");
  expect(radios.map((r) => r.getAttribute("aria-checked"))).toEqual(["false", "true"]);
});

test("with a host one choice sends one set_ui_settings, and settings_changed from the host moves the controls", async () => {
  history.replaceState(null, "", "/settings.html#appearance");
  const { el, transport } = await mount(withHost());
  expect(side(el, "Theme", "Follow the computer")?.getAttribute("aria-pressed")).toBe("true");
  await press(side(el, "Theme", "Night"));
  expect(transport.only("next_command")).toEqual([
    { call: "next_command", command: { set_ui_settings: { patch: { theme: "dark" } } } },
  ]);
  expect(html().dataset["theme"]).toBe("night");
  // pressing the side already chosen sends nothing more
  await press(side(el, "Theme", "Night"));
  expect(transport.only("next_command")).toHaveLength(1);

  // the other window chose Day and 19: the host says so, and this window follows with nothing sent
  await act(async () =>
    transport.settingsChanged({ ui: { ...HOST.ui, theme: "light", font_size: 19 }, locked: [] }),
  );
  expect(side(el, "Theme", "Day")?.getAttribute("aria-pressed")).toBe("true");
  expect(el.querySelector('[role="combobox"]')?.textContent).toContain("19");
  expect(html().dataset["textSize"]).toBe("19");
  expect(transport.only("next_command")).toHaveLength(1);
});

test("a key the host locks disables its row, which says who manages it and sends nothing", async () => {
  history.replaceState(null, "", "/settings.html#appearance");
  const { el, transport } = await mount(withHost({ ...HOST, locked: ["ui.theme"] }));
  const night = side(el, "Theme", "Night");
  expect(night?.disabled || night?.hasAttribute("data-disabled")).toBe(true);
  const row = night?.closest(".set-row");
  expect(row?.querySelector('[data-testid="managed"]')?.textContent).toBe(
    "Managed by your organisation",
  );
  await act(async () => night?.click());
  expect(transport.only("next_command")).toEqual([]);
  expect(html().dataset["theme"]).toBe("day");
  // only the locked row says so; motion is free and sends
  expect(el.querySelectorAll('[data-testid="managed"]')).toHaveLength(1);
  await press(side(el, "Reduced motion", "On"));
  expect(transport.only("next_command")).toHaveLength(1);

  // a lock lifted by the host frees the row at once
  await act(async () => transport.settingsChanged({ ...HOST, locked: [] }));
  expect(side(el, "Theme", "Night")?.disabled).toBe(false);
  expect(el.querySelector('[data-testid="managed"]')).toBeNull();
});

test("About shows the build's version, and its line beside the section says it too", async () => {
  history.replaceState(null, "", "/settings.html#about");
  const { el } = await mount();
  expect(el.querySelector('[data-testid="settings-version"]')?.textContent).toBe("Version 9.8.7");
  expect(el.querySelector('[aria-current="page"] .set-nav-note')?.textContent).toBe(
    "Version 9.8.7",
  );
});

test("off the Mac the title bar's caption buttons work the window; on the Mac there are none", async () => {
  const controls = {
    minimise: vi.fn(async () => {}),
    toggleMaximise: vi.fn(async () => {}),
    close: vi.fn(async () => {}),
    isMaximised: vi.fn(async () => false),
  };
  const { el, store: made } = await mount(new FakeTransport(), controls);
  await act(async () => made.setPlatform("mac"));
  expect(el.querySelector('[role="group"][aria-label="Window"]')).toBeNull();
  expect(el.querySelector(".room-lights[data-native]")).not.toBeNull();
  await act(async () => made.setPlatform("windows"));
  await act(async () => settled());
  expect(el.querySelector("h1.set-win-title")?.textContent).toBe("Settings");
  await press(el.querySelector('[aria-label="Close"]'));
  expect(controls.close).toHaveBeenCalledTimes(1);
  expect(controls.isMaximised).toHaveBeenCalled();
});

for (const platform of ["mac", "windows"] as const) {
  for (const lang of ["en", "hi"] as const) {
    for (const theme of ["light", "dark"] as const) {
      test(`axe finds nothing on any section, on ${platform}, in ${lang}, ${theme}, with one row locked`, async () => {
        localStorage.setItem(DISPLAY_KEY, JSON.stringify({ language: lang, theme }));
        const { el, store: made } = await mount(
          withHost({ ...HOST, ui: { ...HOST.ui, language: lang, theme }, locked: ["ui.motion"] }),
        );
        await act(async () => made.setPlatform(platform));
        for (const button of el.querySelectorAll<HTMLButtonElement>(".set-nav-row")) {
          await press(button);
          const results = await axe.run(document, { resultTypes: ["violations"] });
          const found = results.violations.map(
            (v) =>
              `${button.textContent}: ${v.id} at ${v.nodes.map((n) => n.target.join(" ")).join(", ")}`,
          );
          expect(found).toEqual([]);
        }
      });
    }
  }
}
