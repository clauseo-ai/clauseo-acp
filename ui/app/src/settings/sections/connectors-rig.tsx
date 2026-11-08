// The settings window over FakeTransport for the Connectors tests (U15): a host with the default settings and the
// tools' switches, an app feed whose catch-up carries `state` (none when null), and helpers to play states, press
// buttons by their words and read what went out. Not a test itself.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { settled } from "@clauseo/components/lib/late.ts";
import { connectDisplay } from "../../display/connect.ts";
import type { DisplayStore } from "../../room/display-store.ts";
import type { Settings } from "../../room/host-next.ts";
import { FakeTransport, type FakeHost } from "../../transport/fake-transport.ts";
import type {
  CommandEnvelope,
  CommandOutcome,
  ConnectorView,
  ConnectorsState,
  PackView,
} from "../../transport/protocol.ts";
import { SettingsApp } from "../settings-app.tsx";

export const PACKS: PackView[] = [
  // gross prices, GST included: 200 credits for ₹23.60 (₹20 and ₹3.60 GST)
  { pack: "p200", credits: 200, paise: 2_360 },
  { pack: "p1k", credits: 1_000, paise: 11_800 },
  { pack: "p10l", credits: 10_00_000, paise: 1_18_00_000 },
];

export const CASE: ConnectorView = {
  id: "case-law",
  name: "Indian Kanoon",
  price_paise: 2_360,
  packs: PACKS,
  status: "not_added",
};

/** A state listing Indian Kanoon with `over`, available and unlocked unless `top` says. */
export const stateOf = (
  over: Partial<ConnectorView> = {},
  top: Partial<Omit<ConnectorsState, "connectors">> = {},
): ConnectorsState => ({
  available: true,
  locked: false,
  ...top,
  connectors: [{ ...CASE, ...over }],
});

/** 10,000 credits, worth ₹1,180 gross (11.8 paise a credit, the pack's rate). */
export const SIGNED_IN: ConnectorView["status"] = {
  signed_in: { balance: { credits: 10_000, paise: 1_18_000 }, low: false },
};

const SETTINGS: Settings = {
  ui: { theme: "system", font_size: 16, language: "en", lite: "automatic", motion: "system" },
  locked: [],
  tools: { web_search: true, browser: true },
};

export type Rig = {
  transport: FakeTransport;
  store: DisplayStore;
  el: HTMLDivElement;
  /** Plays a `connectors_changed`, as the host sends it to every window. */
  play: (state: ConnectorsState) => Promise<void>;
  /** The buttons in the pane, by their words. */
  buttons: () => string[];
  /** Presses the pane's button with exactly these words (the first, when several). */
  press: (words: string) => Promise<void>;
  /** Opens a section by its name on the side. */
  side: (name: string) => Promise<void>;
  /** The side's note under a section's name. */
  note: (name: string) => string | undefined;
  /** The pane's text. */
  text: () => string;
  unmount: () => Promise<void>;
};

/** The settings window, open at `hash` (none when left out), its catch-up carrying `state` (none when null). */
export async function mountSettings({
  state = null,
  hash = "",
  dispatch,
}: {
  state?: ConnectorsState | null;
  hash?: string;
  /** Answers each dispatch; done when left out. */
  dispatch?: (command: CommandEnvelope) => CommandOutcome | Promise<CommandOutcome>;
} = {}): Promise<Rig> {
  history.replaceState(null, "", `/settings.html${hash}`);
  const host: FakeHost = {
    next: {
      query: (query) => {
        if (query === "get_settings") return { settings: SETTINGS };
        throw new Error("not scripted");
      },
      first: () => (state === null ? [] : [{ connectors_changed: state }]),
    },
  };
  if (dispatch !== undefined) host.dispatch = dispatch;
  const transport = new FakeTransport(host);
  const store = connectDisplay(transport).store;
  const el = document.createElement("div");
  document.body.append(el);
  const root: Root = createRoot(el);
  await act(async () =>
    root.render(<SettingsApp store={store} transport={transport} windowControls={null} />),
  );
  await act(async () => settled());
  await act(async () => settled());
  const pane = () => el.querySelector("main.set-pane") as HTMLElement;
  const navRow = (name: string) =>
    [...el.querySelectorAll<HTMLButtonElement>(".set-nav-row")].find(
      (b) => b.querySelector(".set-nav-name")?.textContent === name,
    );
  const findButton = (words: string) =>
    [...pane().querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent === words);
  return {
    transport,
    store,
    el,
    play: async (next) => {
      await act(async () => transport.appEvent({ connectors_changed: next }));
      await act(async () => settled());
    },
    buttons: () =>
      [...pane().querySelectorAll<HTMLButtonElement>("button")].map((b) => b.textContent ?? ""),
    press: async (words) => {
      const button = findButton(words);
      if (button === undefined) throw new Error(`no button "${words}"`);
      await act(async () => button.click());
      await act(async () => settled());
    },
    side: async (name) => {
      const row = navRow(name);
      if (row === undefined) throw new Error(`no section "${name}"`);
      await act(async () => row.click());
      await act(async () => settled());
    },
    note: (name) => navRow(name)?.querySelector(".set-nav-note")?.textContent ?? undefined,
    text: () => pane().textContent ?? "",
    unmount: async () => {
      await act(async () => root.unmount());
      el.remove();
      store.dispose();
    },
  };
}
