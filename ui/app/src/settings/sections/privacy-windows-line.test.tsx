// @vitest-environment happy-dom
// Settings › Privacy on Windows keeps the room notice's words readable for good: UC-820, then UC-850 as its own
// sentence, under What leaves this computer, whatever the Safe workspace; on the Mac neither.
import { settled } from "@clauseo/components/lib/late.ts";
import type { Platform } from "@clauseo/components/lib/display.ts";
import { pw, WINDOWS_LINE, WINDOWS_STRICT } from "@clauseo/components/lib/words-protection.ts";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { connectDisplay } from "../../display/connect.ts";
import type { DisplayStore } from "../../room/display-store.ts";
import type { Settings } from "../../room/host-next.ts";
import { stubFonts } from "../../room/thread/testing.ts";
import { FakeTransport } from "../../transport/fake-transport.ts";
import { SettingsApp } from "../settings-app.tsx";

vi.mock("../../transport/window-title.ts", () => ({ setWindowTitle: () => {} }));
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
stubFonts();

let host: HTMLDivElement | null = null;
let root: Root | null = null;
let store: DisplayStore | null = null;

beforeEach(() => history.replaceState(null, "", "/settings.html#privacy"));
afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  store?.dispose();
  root = null;
  host = null;
  store = null;
});

const SETTINGS: Settings = {
  ui: { theme: "system", font_size: 16, language: "en", lite: "automatic", motion: "system" },
  locked: [],
};

async function mount(platform: Platform): Promise<HTMLElement> {
  const transport = new FakeTransport({
    next: {
      query: (query) => {
        if (query === "get_settings") return { settings: SETTINGS };
        throw new Error("not scripted");
      },
    },
  });
  const made = connectDisplay(transport).store;
  store = made;
  const el = document.createElement("div");
  host = el;
  document.body.append(el);
  root = createRoot(el);
  await act(async () =>
    root?.render(<SettingsApp store={made} transport={transport} windowControls={null} />),
  );
  await act(async () => made.setPlatform(platform));
  for (let i = 0; i < 3; i += 1) await act(async () => settled());
  return el;
}

const line = (el: HTMLElement) =>
  el.querySelector<HTMLElement>('[data-testid="privacy-windows-line"]');

test("on Windows, Privacy says UC-820 then UC-850, after What leaves this computer", async () => {
  const el = await mount("windows");
  expect(el.querySelector(".set-pane h2")?.textContent).toBe("Privacy");
  expect(line(el)?.textContent).toBe(`${pw("en", WINDOWS_LINE)} ${pw("en", WINDOWS_STRICT)}`);
  const groups = [...el.querySelectorAll(".set-pane .set-group")];
  const at = groups.findIndex((g) => g.contains(line(el)));
  expect(at).toBeGreaterThan(0);
  expect(groups[at - 1]?.textContent).toContain("What leaves this computer");
});

test.each(["mac", "linux"] as const)(
  "on %s, Privacy says neither UC-820 nor UC-850",
  async (platform) => {
    const el = await mount(platform);
    expect(el.querySelector(".set-pane h2")?.textContent).toBe("Privacy");
    expect(line(el)).toBeNull();
    expect(el.textContent).not.toContain(pw("en", WINDOWS_LINE));
    expect(el.textContent).not.toContain(pw("en", WINDOWS_STRICT));
  },
);
