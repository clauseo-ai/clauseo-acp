// The settings window over FakeTransport for Settings › Tools' tests: open at #tools, with a fake host handed over
// (tools-fake.ts) or, with none, the real seam over the transport's app feed (`first` its first frames, `dispatch`
// each command's answer), and helpers to press, type, save and read. Not a test itself.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { settled } from "@clauseo/components/lib/late.ts";
import { connectDisplay } from "../../display/connect.ts";
import type { AppEvent, Settings } from "../../room/host-next.ts";
import { FakeTransport } from "../../transport/fake-transport.ts";
import type { CommandEnvelope, CommandOutcome } from "../../transport/protocol.ts";
import { SettingsApp } from "../settings-app.tsx";
import { local, type FakeTools, type FakeView } from "./tools-fake.ts";
import { provideTools } from "./tools-model.ts";

const SETTINGS: Settings = {
  ui: { theme: "system", font_size: 16, language: "en", lite: "automatic", motion: "system" },
  locked: [],
};

export const PLAIN =
  "Tools run on this computer with the same access you have: your files, your programs and the internet. Add only tools you trust.";

export const THREE: FakeView = {
  control: "lawyer",
  servers: [
    local("court-diary", "/opt/homebrew/bin/npx", { args: ["-y", "court-diary-mcp"] }),
    local("filings", "/Users/r/tools/filings-server", { enabled: false }),
    local("notes", "/usr/local/bin/uvx", { env: [{ name: "NOTES_TOKEN", saved: true }] }),
  ],
};

export type Rig = Awaited<ReturnType<typeof mount>>;

export async function mount(
  tools: FakeTools | null,
  {
    safe = false,
    first = [],
    dispatch,
  }: {
    safe?: boolean;
    /** The app feed's first frames, for a host without a fake. */
    first?: AppEvent[];
    dispatch?: (envelope: CommandEnvelope) => CommandOutcome;
  } = {},
) {
  history.replaceState(null, "", "/settings.html#tools");
  const transport = new FakeTransport({
    next: {
      query: (query) => {
        if (query === "get_settings") return { settings: SETTINGS };
        throw new Error("not scripted");
      },
      first: () => first,
    },
    ...(dispatch === undefined ? {} : { dispatch }),
  });
  if (tools !== null) provideTools(transport, tools);
  // a build with the Safe workspace: the transport carries its wire, as sections.tsx reads it
  if (safe) Object.defineProperty(transport, "protectionWire", { value: true });
  const store = connectDisplay(transport).store;
  const el = document.createElement("div");
  document.body.append(el);
  const root: Root = createRoot(el);
  const settle = async () => {
    for (let i = 0; i < 3; i += 1) await act(async () => settled());
  };
  await act(async () =>
    root.render(<SettingsApp store={store} transport={transport} windowControls={null} />),
  );
  await settle();
  const pane = () => el.querySelector("main.set-pane") as HTMLElement;
  const q = <E extends Element = HTMLElement>(sel: string) => pane().querySelector<E>(sel);
  const button = (words: string) =>
    [...document.querySelectorAll<HTMLButtonElement>("button")].find(
      (b) => b.textContent === words,
    );
  const type = async (target: HTMLInputElement | HTMLTextAreaElement, text: string) => {
    const proto =
      target instanceof HTMLTextAreaElement
        ? HTMLTextAreaElement.prototype
        : HTMLInputElement.prototype;
    await act(async () => {
      Reflect.set(proto, "value", text, target);
      target.dispatchEvent(new Event("input", { bubbles: true }));
    });
  };
  return {
    el,
    transport,
    pane,
    q,
    text: () => pane()?.textContent ?? "",
    nav: () =>
      [...el.querySelectorAll(".set-nav-row .set-nav-name")].map((n) => n.textContent ?? ""),
    rows: () =>
      [...pane().querySelectorAll('[data-testid="tools-row"]')].map((r) => r.textContent ?? ""),
    errors: () =>
      [...pane().querySelectorAll('[data-testid="tools-error"]')].map((r) => r.textContent ?? ""),
    press: async (words: string) => {
      const b = button(words);
      if (b === undefined) throw new Error(`no button "${words}"`);
      await act(async () => b.click());
      await settle();
    },
    click: async (target: HTMLElement | null) => {
      if (target === null) throw new Error("nothing to click");
      await act(async () => target.click());
      await settle();
    },
    has: (words: string) => button(words) !== undefined,
    type,
    field: (id: string) => q<HTMLInputElement>(`[data-testid="${id}"]`)!,
    save: async () => {
      await act(async () => q<HTMLFormElement>('[data-testid="tools-form"]')!.requestSubmit());
      await settle();
    },
    unmount: async () => {
      await act(async () => root.unmount());
      el.remove();
      store.dispose();
    },
  };
}
