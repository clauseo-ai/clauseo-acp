// @vitest-environment happy-dom
// The settings window's frame: the sections down the side, the open one marked; the open section's name as the
// pane's h2 under the window's one h1; the pane back at its top on a new section; the caption buttons off the Mac and
// the corner lights on it.
import { act, useState, type ReactNode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, test, vi } from "vitest";
import { SettingsWindow, type SettingsNavItem } from "./settings-window.tsx";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement | null = null;
let root: Root | null = null;

async function mount(node: ReactNode) {
  host ??= document.createElement("div");
  document.body.append(host);
  root ??= createRoot(host);
  await act(async () => root?.render(node));
  return host;
}

afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  root = null;
  host = null;
  vi.restoreAllMocks();
});

const SECTIONS: SettingsNavItem[] = [
  { id: "language", name: "Language", note: "English" },
  { id: "appearance", name: "Appearance", note: "Day, text size 16" },
  { id: "about", name: "About" },
];

const frame = (over: Partial<Parameters<typeof SettingsWindow>[0]> = {}) => (
  <SettingsWindow
    platform="mac"
    title="Settings"
    sections={SECTIONS}
    current="language"
    onSection={() => {}}
    lead="The language of every word on screen."
    {...over}
  >
    <p>body</p>
  </SettingsWindow>
);

test("the side lists each section with its note, marks the open one, and a press names the section", async () => {
  const onSection = vi.fn();
  const el = await mount(frame({ onSection }));
  const nav = el.querySelector('nav[aria-label="Sections"]');
  const rows = [...(nav?.querySelectorAll<HTMLButtonElement>("button.set-nav-row") ?? [])];
  expect(rows.map((r) => r.querySelector(".set-nav-name")?.textContent)).toEqual([
    "Language",
    "Appearance",
    "About",
  ]);
  expect(rows.map((r) => r.querySelector(".set-nav-note")?.textContent ?? null)).toEqual([
    "English",
    "Day, text size 16",
    null,
  ]);
  expect(rows.map((r) => r.getAttribute("aria-current"))).toEqual(["page", null, null]);
  await act(async () => rows[1]?.click());
  expect(onSection).toHaveBeenCalledWith("appearance");
});

test("on the Mac the side's head is the window's one h1, the pane's head the section's h2, with its lead and body", async () => {
  const el = await mount(frame());
  const win = el.querySelector(".set-win");
  // a real window with its native title: no dialog role; the pane is the main landmark, named by its h2
  expect(win?.getAttribute("role")).toBeNull();
  expect(win?.hasAttribute("aria-modal")).toBe(false);
  expect(win?.getAttribute("data-platform")).toBe("mac");
  const main = el.querySelector("main.set-pane");
  expect(main).not.toBeNull();
  expect(document.getElementById(main?.getAttribute("aria-labelledby") ?? "")?.textContent).toBe(
    "Language",
  );
  expect([...el.querySelectorAll("h1")].map((h) => h.textContent)).toEqual(["Settings"]);
  expect(el.querySelector(".set-side h1.set-side-title")).not.toBeNull();
  expect(el.querySelector(".set-pane h2.set-pane-title")?.textContent).toBe("Language");
  expect(el.querySelector(".set-pane-lead")?.textContent).toBe(
    "The language of every word on screen.",
  );
  expect(el.querySelector(".set-pane-inner > p:last-child")?.textContent).toBe("body");
  // the corner lights, and no caption buttons
  expect(el.querySelector(".set-win-lights .room-lights")).not.toBeNull();
  expect(el.querySelector(".room-caption")).toBeNull();
});

test("a section with no lead has none", async () => {
  const el = await mount(frame({ current: "about", lead: undefined }));
  expect(el.querySelector(".set-pane-title")?.textContent).toBe("About");
  expect(el.querySelector(".set-pane-lead")).toBeNull();
});

test("off the Mac the title bar holds the title as the h1 and the caption buttons, wired to the window", async () => {
  for (const platform of ["windows", "linux"] as const) {
    const onClose = vi.fn();
    const el = await mount(frame({ platform, caption: { onClose } }));
    expect(el.querySelector("header.set-win-bar h1.set-win-title")?.textContent).toBe("Settings");
    expect(el.querySelector(".set-side-title")).toBeNull();
    expect(el.querySelector(".set-win-lights")).toBeNull();
    // all three buttons, as the design's window has them
    expect(el.querySelectorAll(".room-caption button")).toHaveLength(3);
    await act(async () => el.querySelector<HTMLButtonElement>('[aria-label="Close"]')?.click());
    expect(onClose).toHaveBeenCalledTimes(1);
  }
});

test("a new section starts the pane at its top", async () => {
  const el = await mount(frame());
  const pane = el.querySelector<HTMLElement>(".set-pane");
  const scrolled = vi.fn();
  if (pane !== null) pane.scrollTo = scrolled as typeof pane.scrollTo;
  await mount(frame({ current: "appearance" }));
  expect(scrolled).toHaveBeenCalledWith({ top: 0 });
  expect(el.querySelector(".set-pane-title")?.textContent).toBe("Appearance");
});

test("a section given an icon shows it before its name, hidden from assistive technology; one without shows none", async () => {
  const el = await mount(
    frame({
      sections: [
        { id: "language", name: "Language", note: "English", icon: <svg data-mark="" /> },
        { id: "about", name: "About" },
      ],
    }),
  );
  const [withIcon, without] = [...el.querySelectorAll(".set-nav-row")];
  expect(withIcon?.querySelector(".set-nav-icon")?.getAttribute("aria-hidden")).toBe("true");
  expect(withIcon?.querySelector(".set-nav-icon [data-mark]")).not.toBeNull();
  expect(withIcon?.querySelector(".set-nav-name")?.textContent).toBe("Language");
  expect(without?.querySelector(".set-nav-icon")).toBeNull();
});

test("each rise of focusHead focuses the open section's head; none at 0, and a section change alone moves no focus", async () => {
  const el = await mount(frame());
  const head = () => el.querySelector("h2");
  expect(head()?.tabIndex).toBe(-1);
  expect(document.activeElement).not.toBe(head());
  await mount(frame({ current: "appearance" }));
  expect(document.activeElement).not.toBe(head());
  await mount(frame({ current: "about", focusHead: 1 }));
  expect(head()?.textContent).toBe("About");
  expect(document.activeElement).toBe(head());
  (document.activeElement as HTMLElement).blur();
  await mount(frame({ current: "language", focusHead: 2 }));
  expect(document.activeElement).toBe(head());
  expect(head()?.textContent).toBe("Language");
});

function Gone() {
  const [here, setHere] = useState(true);
  return here ? (
    <button type="button" data-testid="gone" onClick={() => setHere(false)}>
      Sign out
    </button>
  ) : (
    <p>Signed out.</p>
  );
}

test("a control in the pane that takes itself away leaves focus on the section's head, not the page", async () => {
  const node = await mount(
    <SettingsWindow
      platform="mac"
      title="Settings"
      sections={SECTIONS}
      current="language"
      onSection={() => {}}
    >
      <Gone />
    </SettingsWindow>,
  );
  const button = node.querySelector<HTMLElement>('[data-testid="gone"]')!;
  button.focus();
  await act(async () => button.click());
  await act(async () => new Promise((resolve) => setTimeout(resolve, 0)));
  const head = node.querySelector<HTMLElement>(".set-pane h2");
  expect(head?.getAttribute("tabindex")).toBe("-1");
  expect(document.activeElement).toBe(head);
});
