// Shared setup for the Safe workspace's tests in the settings window (UC): the window over a FakeTransport with the
// protection wire, open at #workspace, the updater's view and the window's language as each test scripts them, a held
// protection host, and helpers to find and press what shows. Not shipped: only tests import it.
import { settled } from "@clauseo/components/lib/late.ts";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { vi } from "vitest";
import { connectDisplay } from "../../display/connect.ts";
import { DEFAULT_UI, type DisplayStore } from "../../room/display-store.ts";
import { view } from "../../room/protection/fixtures.ts";
import { FakeTransport, type FakeProtectionHost } from "../../transport/fake-transport.ts";
import type { Protection, ProtectionView } from "../../transport/protocol.ts";
import { SettingsApp } from "../settings-app.tsx";

let host: HTMLDivElement | null = null;
let root: Root | null = null;
let store: DisplayStore | null = null;

/** What a test scripts: the updater's view `get_update` answers (undefined: not scripted, refused), and the
 * language the settings window's ui settings name. */
export const scripted: { update: unknown; language: "en" | "hi" } = {
  update: undefined,
  language: "en",
};

/** Unmounts the window, for a second mount in the same test. */
export async function unmount() {
  await act(async () => root?.unmount());
  host?.remove();
  store?.dispose();
  host = null;
  root = null;
  store = null;
  document.body.replaceChildren();
}

/** After each test: the window gone, mocks restored, nothing scripted. */
export async function reset() {
  await unmount();
  vi.restoreAllMocks();
  scripted.update = undefined;
  scripted.language = "en";
}

export async function settle() {
  for (let i = 0; i < 4; i += 1) {
    await act(async () => new Promise((resolve) => setTimeout(resolve, 0)));
    await act(async () => settled());
  }
}

type Answer = { resolve: () => void; reject: (e: unknown) => void };

/** The settings window at the Safe workspace, on `platform`, over `protection` (none: no wire). */
export async function mount(
  protection: FakeProtectionHost | undefined,
  platform: "mac" | "windows" | "linux" = "windows",
  hash = "#workspace",
) {
  const settings = { ui: { ...DEFAULT_UI, language: scripted.language }, locked: [] };
  const transport = new FakeTransport({
    platform,
    query: (query) => {
      if (query === "get_settings") return { settings: settings as never };
      if (query === "get_update" && scripted.update !== undefined)
        return { update: scripted.update as never };
      throw new Error(`not scripted: ${JSON.stringify(query)}`);
    },
    next: { query: () => ({ settings }) as never },
    ...(protection === undefined ? {} : { protection }),
  });
  history.replaceState(null, "", `/settings.html${hash}`);
  store = connectDisplay(transport).store;
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  const made = store;
  await act(async () =>
    root?.render(<SettingsApp store={made} transport={transport} windowControls={null} />),
  );
  await settle();
  return transport;
}

/** A computer at `computer` with `over`; each command held until the test answers it. */
export function held(computer: Protection, over: Partial<ProtectionView> = {}) {
  const answers: Answer[] = [];
  const state = { computer };
  const host: FakeProtectionHost = {
    view: () => view(state.computer, over),
    command: () => new Promise<void>((resolve, reject) => answers.push({ resolve, reject })),
  };
  return { host, answers, state };
}

export const one = (id: string) => document.querySelector<HTMLElement>(`[data-testid="${id}"]`);
export const block = () => one("settings-workspace");
export const text = () => block()?.textContent ?? "";
export async function press(el: HTMLElement | null) {
  if (el === null) throw new Error("nothing to press");
  await act(async () => el.click());
  await settle();
}
