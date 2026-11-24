// @vitest-environment happy-dom
// Settings, Privacy: the Permissions group (issue 5, #1303), the first-start step's rows for a lawyer who already has
// matters: no group with no row; one row, the group headed Permissions with that row, its Allow asking the host once.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { settled } from "@clauseo/components/lib/late.ts";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { connectDisplay } from "../../display/connect.ts";
import type { DisplayStore } from "../../room/display-store.ts";
import type { Settings } from "../../room/host-next.ts";
import {
  providePermissions,
  type PermissionRow,
  type PermissionsHost,
} from "../../room/first-run/permissions-host.ts";
import { stubFonts } from "../../room/thread/testing.ts";
import { FakeTransport } from "../../transport/fake-transport.ts";
import { SettingsApp } from "../settings-app.tsx";

vi.mock("../../transport/window-title.ts", () => ({ setWindowTitle: () => {} }));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
stubFonts();

const items = new Map<string, string>();
vi.stubGlobal("localStorage", {
  getItem: (key: string) => items.get(key) ?? null,
  setItem: (key: string, value: string) => void items.set(key, value),
  removeItem: (key: string) => void items.delete(key),
  clear: () => items.clear(),
});

let host: HTMLDivElement | null = null;
let root: Root | null = null;
let store: DisplayStore | null = null;

beforeEach(() => {
  localStorage.clear();
  history.replaceState(null, "", "/settings.html#privacy");
});

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
const KEYS: PermissionRow = {
  id: "keychain",
  state: "would_ask",
  folder: null,
  settings_path: null,
  note: null,
};

async function mount(permissions: PermissionsHost | undefined) {
  const transport = new FakeTransport({
    next: {
      query: (query) => {
        if (query === "get_settings") return { settings: SETTINGS };
        throw new Error("not scripted");
      },
    },
  });
  if (permissions !== undefined) providePermissions(transport, permissions);
  const made = connectDisplay(transport).store;
  store = made;
  const el = document.createElement("div");
  host = el;
  document.body.append(el);
  root = createRoot(el);
  await act(async () =>
    root?.render(<SettingsApp store={made} transport={transport} windowControls={null} />),
  );
  await act(async () => made.setPlatform("mac"));
  await act(async () => settled());
  return el;
}

const group = (el: HTMLElement) => el.querySelector('[data-testid="settings-permissions"]');

test("no row: no Permissions group in Privacy, and none where the window has no permissions host", async () => {
  const el = await mount({ list: async () => [], ask: async () => KEYS });
  expect(el.querySelector(".set-pane h2")?.textContent).toBe("Privacy");
  expect(group(el)).toBeNull();
  await act(async () => root?.unmount());
  const bare = await mount(undefined);
  expect(bare.querySelector(".set-pane h2")?.textContent).toBe("Privacy");
  expect(group(bare)).toBeNull();
});

test("one row: the group headed Permissions, first in Privacy, with that row; Allow asks the host once", async () => {
  const ask = vi.fn(async () => ({ ...KEYS, state: "granted" as const }));
  const el = await mount({ list: async () => [KEYS], ask });
  const found = group(el);
  expect(found?.querySelector("h3")?.textContent).toBe("Permissions");
  // the first-start step's own heading is the group's here
  expect(found?.textContent).not.toContain("Before your first matter");
  const rows = [...(found?.querySelectorAll('[data-testid="permission-row"]') ?? [])];
  expect(rows.map((r) => r.getAttribute("data-id"))).toEqual(["keychain"]);
  expect(found?.textContent).toContain("Your keychain");
  // first in the pane, above what leaves this computer
  expect(
    el.querySelector(".set-pane .set-group")?.closest('[data-testid="settings-permissions"]'),
  ).toBe(found);
  await act(async () =>
    found?.querySelector<HTMLElement>('[data-testid="permission-allow"]')?.click(),
  );
  await act(async () => settled());
  expect(ask).toHaveBeenCalledTimes(1);
  expect(group(el)?.querySelector('[data-testid="permission-granted"]')?.textContent).toBe(
    "Allowed",
  );
});
