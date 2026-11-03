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
import {
  IconAbout,
  IconAppearance,
  IconLanguage,
  IconPlanLimit,
  IconSafeWorkspace,
} from "@clauseo/components/icons/icons.tsx";
import { iconIn } from "@clauseo/components/icons/icon-in.tsx";

vi.mock("./version.ts", () => ({ APP_VERSION: "9.8.7" }));
const setWindowTitle = vi.hoisted(() => vi.fn<(title: string) => void>());
vi.mock("../transport/window-title.ts", () => ({ setWindowTitle }));

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
  setWindowTitle.mockClear();
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

/** A transport whose host has H7's settings, answering get_settings with `settings`, and reports `releaseDate`. */
const withHost = (settings: Settings = HOST, releaseDate: string | null = null) =>
  new FakeTransport({
    releaseDate,
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

test("the sections are Plan and usage, Language, Appearance, Privacy and About, in the design's order; no later section renders", async () => {
  const { el } = await mount();
  expect(navNames(el)).toEqual(["Plan and usage", "Language", "Appearance", "Privacy", "About"]);
  for (const later of ["Plans", "Indian Kanoon", "Matters", "Safe workspace", "Updates"])
    expect(el.textContent).not.toContain(later);
  // the first opens, as the pane's h2, under the window's one h1
  expect(el.querySelector('[aria-current="page"] .set-nav-name')?.textContent).toBe(
    "Plan and usage",
  );
  expect(el.querySelector(".set-pane h2")?.textContent).toBe("Plan and usage");
  expect(el.querySelectorAll("h1")).toHaveLength(1);
  expect(document.title).toBe("Settings");
  expect(el.querySelector(".set-pane-lead")?.textContent).toBe(
    "What each plan has left, and when it resets.",
  );
});

test("the native window title names the window in its language, at open and on every change of language", async () => {
  localStorage.setItem(DISPLAY_KEY, JSON.stringify({ language: "en" }));
  history.replaceState(null, "", "/settings.html#language");
  const { el } = await mount();
  expect(setWindowTitle.mock.calls).toEqual([["Settings"]]);
  await press(side(el, "Language", "हिन्दी"));
  expect(setWindowTitle).toHaveBeenLastCalledWith("सेटिंग्स");
  expect(document.title).toBe("सेटिंग्स");
  // the switch now names itself in Hindi
  await press(side(el, "भाषा", "English"));
  expect(setWindowTitle).toHaveBeenLastCalledWith("Settings");
  expect(setWindowTitle).toHaveBeenCalledTimes(3);
});

test("a window opened in Hindi gets its Hindi title at once", async () => {
  localStorage.setItem(DISPLAY_KEY, JSON.stringify({ language: "hi" }));
  await mount();
  expect(setWindowTitle.mock.calls).toEqual([["सेटिंग्स"]]);
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
  expect(el.querySelector(".set-pane h2")?.textContent).toBe("Plan and usage");
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

test("every locked control names its managed line in aria-describedby: switches, the select and the radio group", async () => {
  history.replaceState(null, "", "/settings.html#appearance");
  const all = ["ui.theme", "ui.font_size", "ui.lite", "ui.motion", "ui.language"];
  const { el } = await mount(withHost({ ...HOST, locked: all }));
  /** The text of the elements an element's aria-describedby names, joined. */
  const described = (node: Element | null | undefined) =>
    (node?.getAttribute("aria-describedby") ?? "")
      .split(" ")
      .filter((id) => id !== "")
      .map((id) => document.getElementById(id)?.textContent ?? "")
      .join(" ");
  const managed = "Managed by your organisation";
  expect(el.querySelectorAll('[data-testid="managed"]')).toHaveLength(4);
  expect(described(el.querySelector('[aria-label="Theme"]'))).toBe(managed);
  expect(described(el.querySelector('[aria-label="Reduced motion"]'))).toBe(managed);
  expect(described(el.querySelector('[role="combobox"]'))).toBe(managed);
  // the radio group keeps its own line first
  expect(described(el.querySelector('[role="radiogroup"]'))).toBe(
    `Takes away every shadow and all motion. ${managed}`,
  );
  await section(el, "Language");
  expect(described(el.querySelector('[aria-label="Language"]'))).toBe(managed);
});

test("a control that is not locked names no managed line", async () => {
  history.replaceState(null, "", "/settings.html#appearance");
  const { el } = await mount(withHost());
  expect(el.querySelector('[aria-label="Theme"]')?.hasAttribute("aria-describedby")).toBe(false);
  expect(el.querySelector('[role="combobox"]')?.hasAttribute("aria-describedby")).toBe(false);
});

test("Appearance opens with the design's preview: three papers as filed and the answer's line, hidden from assistive technology", async () => {
  history.replaceState(null, "", "/settings.html#appearance");
  const { el } = await mount();
  const preview = el.querySelector(".set-preview");
  expect(preview?.getAttribute("aria-hidden")).toBe("true");
  expect(preview?.closest("section")?.querySelector("h3")?.textContent).toBe("How it looks");
  expect([...el.querySelectorAll(".set-preview-title")].map((t) => t.textContent)).toEqual([
    "Form DIR-12",
    "Board minutes",
    "Legal notice",
  ]);
  expect(el.querySelector(".set-preview-work")?.textContent).toBe(
    "A strong case, because the complaint names Mr. Sharma only as a director.",
  );
  await section(el, "Language");
  await press(side(el, "Language", "हिन्दी"));
  await section(el, "दिखावट");
  expect(el.querySelector(".set-preview-work")?.textContent).toBe(
    "मामला मज़बूत है, क्योंकि शिकायत में श्री शर्मा को केवल निदेशक के रूप में नामित किया गया है।",
  );
  // papers keep their filed titles
  expect(el.querySelector(".set-preview-title")?.textContent).toBe("Form DIR-12");
});

test("About shows the build's version, and its line beside the section says it too", async () => {
  history.replaceState(null, "", "/settings.html#about");
  const { el } = await mount();
  expect(el.querySelector('[data-testid="settings-version"]')?.textContent).toBe("Version 9.8.7");
  // Clauseo's lockup above it, named for assistive technology
  const logo = el.querySelector('.set-about svg[role="img"]');
  expect(logo?.getAttribute("aria-label")).toBe("Clauseo");
  expect(logo?.querySelectorAll("path")).toHaveLength(2);
  expect(el.querySelector('[aria-current="page"] .set-nav-note')?.textContent).toBe(
    "Version 9.8.7",
  );
});

test("About's release line shows the host's release date in the language, with Latin digits", async () => {
  history.replaceState(null, "", "/settings.html#about");
  const { el } = await mount(new FakeTransport({ releaseDate: "2026-11-03" }));
  expect(el.querySelector('[data-testid="settings-release-date"]')?.textContent).toBe(
    "Released 3 November 2026",
  );
  // the version line stays where it was, above it
  expect(el.querySelector('[data-testid="settings-version"]')?.nextElementSibling).toBe(
    el.querySelector('[data-testid="settings-release-date"]'),
  );
});

test("About's release line reads in Hindi when the language is Hindi", async () => {
  history.replaceState(null, "", "/settings.html#about");
  const hi = { ...HOST, ui: { ...HOST.ui, language: "hi" as const } };
  const { el } = await mount(withHost(hi, "2026-11-03"));
  expect(el.querySelector('[data-testid="settings-release-date"]')?.textContent).toBe(
    "3 नवंबर 2026 को जारी",
  );
});

test("About shows no release line where the host has no release date (a dev or CI build)", async () => {
  history.replaceState(null, "", "/settings.html#about");
  const { el } = await mount(new FakeTransport());
  expect(el.querySelector('[data-testid="settings-version"]')).not.toBeNull();
  expect(el.querySelector('[data-testid="settings-release-date"]')).toBeNull();
});

test("About shows no release line for a date that isn't YYYY-MM-DD", async () => {
  history.replaceState(null, "", "/settings.html#about");
  const { el } = await mount(new FakeTransport({ releaseDate: "3 November 2026" }));
  expect(el.querySelector('[data-testid="settings-release-date"]')).toBeNull();
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

test("each section on the side carries its 16 px icon before its name, hidden; the name still names the section", async () => {
  const { el } = await mount();
  const rows = [...el.querySelectorAll<HTMLButtonElement>(".set-nav-row")];
  const icons = [IconPlanLimit, IconLanguage, IconAppearance, IconSafeWorkspace, IconAbout];
  expect(rows).toHaveLength(icons.length);
  rows.forEach((row, i) => {
    const holder = row.querySelector(".set-nav-icon");
    expect(holder?.getAttribute("aria-hidden")).toBe("true");
    expect(iconIn(holder, icons[i] ?? IconAbout, 16)).not.toBeNull();
    expect(row.firstElementChild).toBe(holder);
  });
  expect(navNames(el)).toEqual(["Plan and usage", "Language", "Appearance", "Privacy", "About"]);
});
