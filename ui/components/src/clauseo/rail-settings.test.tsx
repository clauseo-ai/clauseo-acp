// @vitest-environment happy-dom
// The rail's foot: Settings as a button with its shortcut, or a link, or none (moved out of rail.test.tsx, which keeps
// under 600 lines).
import { act, type ReactNode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, describe, expect, test } from "vitest";
import { DEFAULT_DISPLAY, showDisplay } from "../lib/display.ts";
import { Rail } from "./rail.tsx";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement | null = null;
let root: Root | null = null;

async function mount(node: ReactNode) {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root?.render(node));
}

afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  root = null;
  showDisplay({ ...DEFAULT_DISPLAY, motion: "reduced" });
});

const txt = (n: Element | null | undefined) => (n?.textContent ?? "").replace(/\u00a0/g, " ");

describe("Settings at the rail's foot", () => {
  test("with onSettings, Settings is a button that calls it, with the shortcut named and its test id", async () => {
    let pressed = 0;
    await mount(
      <Rail
        open
        groups={[]}
        onSettings={() => (pressed += 1)}
        testIds={{ settings: "open-settings" }}
      />,
    );
    const button = document.querySelector<HTMLButtonElement>("button.room-settings");
    expect(button?.dataset["testid"]).toBe("open-settings");
    expect(button?.type).toBe("button");
    expect(txt(button)).toBe("Settings");
    expect(button?.title).toBe("⌘,");
    expect(button?.getAttribute("aria-keyshortcuts")).toBe("Meta+Comma");
    expect(document.querySelector("a.room-settings")).toBeNull();
    await act(async () => button?.click());
    expect(pressed).toBe(1);
  });

  test("off the Mac the shortcut is Ctrl+,", async () => {
    showDisplay({ ...DEFAULT_DISPLAY, platform: "windows", motion: "reduced" });
    await mount(<Rail open groups={[]} onSettings={() => {}} />);
    const button = document.querySelector<HTMLButtonElement>("button.room-settings");
    expect(button?.title).toBe("Ctrl+,");
    expect(button?.getAttribute("aria-keyshortcuts")).toBe("Control+Comma");
  });

  test("with only settings it stays a link, and with neither the foot has no Settings", async () => {
    await mount(<Rail open groups={[]} settings="#settings" />);
    expect(document.querySelector("a.room-settings")?.getAttribute("href")).toBe("#settings");
    expect(document.querySelector("button.room-settings")).toBeNull();
    await act(async () => root?.unmount());
    host?.remove();
    await mount(<Rail open groups={[]} scratch={{}} />);
    expect(document.querySelector(".room-settings")).toBeNull();
    expect(document.querySelector(".room-rail-foot")).toBeNull();
  });
});

describe("the Test build label at the rail's foot", () => {
  test("a test build shows it beside Settings, as a tag that is no control", async () => {
    showDisplay({ ...DEFAULT_DISPLAY, testBuild: true, motion: "reduced" });
    await mount(
      <Rail open groups={[]} onSettings={() => {}} testIds={{ settings: "open-settings" }} />,
    );
    const tag = document.querySelector<HTMLElement>(".room-rail-foot [data-testid='test-build']");
    expect(txt(tag)).toBe("Test build");
    expect(tag?.closest("button")).toBeNull();
    expect(tag?.getAttribute("role")).toBeNull();
    expect(document.querySelector(".room-settings")?.previousElementSibling).toBe(tag);
  });

  test("a test build with no Settings still shows it, at the foot", async () => {
    showDisplay({ ...DEFAULT_DISPLAY, testBuild: true, motion: "reduced" });
    await mount(<Rail open groups={[]} scratch={{}} />);
    expect(txt(document.querySelector(".room-rail-foot [data-testid='test-build']"))).toBe(
      "Test build",
    );
  });

  test("a build that is not a test build shows no label, with or without Settings", async () => {
    await mount(<Rail open groups={[]} onSettings={() => {}} />);
    expect(document.querySelector("[data-testid='test-build']")).toBeNull();
    expect(document.querySelector(".room-settings")).not.toBeNull();
  });

  test("the label reads in Hindi in a Hindi display", async () => {
    showDisplay({ ...DEFAULT_DISPLAY, lang: "hi", testBuild: true, motion: "reduced" });
    await mount(<Rail open groups={[]} onSettings={() => {}} />);
    expect(txt(document.querySelector("[data-testid='test-build']"))).toBe("टेस्ट बिल्ड");
  });
});
