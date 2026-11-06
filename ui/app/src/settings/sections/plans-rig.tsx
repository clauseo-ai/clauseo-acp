// Shared setup for Plans' tests (UD2): the settings window over a FakeTransport that carries the setup wire, open at
// Plans, with the host's settings (locks, the firm's plan) and helpers to play SetupEvents and press what shows. Not
// shipped: only tests import it.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { settled } from "@clauseo/components/lib/late.ts";
import { connectDisplay } from "../../display/connect.ts";
import { DEFAULT_UI, type DisplayStore } from "../../room/display-store.ts";
import { FakeTransport } from "../../transport/fake-transport.ts";
import type {
  AgentId,
  InstallProgress,
  SettingsView,
  SetupCommand,
  SetupEvent,
  SignIn,
} from "../../transport/protocol.ts";
import { SettingsApp } from "../settings-app.tsx";

export const CLAUDE: AgentId = "claude-acp";
export const CODEX: AgentId = "codex-acp";
export const AT = Date.UTC(2026, 10, 6, 11, 2);
export const READY: InstallProgress = { ready: { version: "0.1.0" } };

export const install = (agent: AgentId, state: InstallProgress): SetupEvent => ({
  agent_install_progress: { agent, state },
});
export const signIn = (agent: AgentId, state: SignIn): SetupEvent => ({
  sign_in_changed: { agent, state },
});
export const info = (keys = true): SetupEvent => ({ setup_info: { keys_available: keys } });
export const signed = (
  over: Partial<NonNullable<Extract<SignIn, { signed_in: unknown }>["signed_in"]>> = {},
): SignIn => ({
  signed_in: {
    email: "r.mehra@mehrachambers.in",
    plan: "Claude Max",
    method: "plan",
    source: "own",
    at: AT,
    ...over,
  },
});

let host: HTMLDivElement | null = null;
let root: Root | null = null;
let store: DisplayStore | null = null;

export async function unmount() {
  await act(async () => root?.unmount());
  host?.remove();
  store?.dispose();
  host = null;
  root = null;
  store = null;
}

export type PlansRig = {
  /** The setup events the feed starts with, after SetupInfo. */
  first?: SetupEvent[];
  keys?: boolean;
  locked?: string[];
  plans?: SettingsView["plans"];
  /** Answers each setup command; throwing refuses it. */
  command?: (command: SetupCommand) => void;
};

/** The settings window at Plans over a fake host with the setup wire. */
export async function mountPlans(o: PlansRig = {}): Promise<FakeTransport> {
  const transport = new FakeTransport({
    platform: "mac",
    query: (query) => {
      if (query === "get_settings")
        return {
          settings: {
            ui: {
              theme: "system",
              font_size: 16,
              language: "en",
              lite: "automatic",
              motion: "system",
            },
            tools: { web_search: true, browser: true },
            privacy: { counts: true, crash_reports: false, counts_started: true },
            locked: o.locked ?? [],
            ...(o.plans === undefined ? {} : { plans: o.plans }),
          } as SettingsView,
        };
      throw new Error(`not scripted: ${JSON.stringify(query)}`);
    },
    // the display store's settings, as the room's NextHost answers them (states-settings.tsx)
    next: {
      query: (query) => {
        if (query === "get_settings")
          return {
            settings: {
              ui: DEFAULT_UI,
              locked: o.locked ?? [],
              ...(o.plans === undefined ? {} : { plans: o.plans }),
            },
          };
        throw new Error("not scripted");
      },
    },
    setup: {
      first: () => [info(o.keys ?? true), ...(o.first ?? [])],
      ...(o.command === undefined ? {} : { command: o.command }),
    },
  });
  history.replaceState(null, "", "/settings.html#plans");
  const made = connectDisplay(transport).store;
  store = made;
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () =>
    root?.render(<SettingsApp store={made} transport={transport} windowControls={null} />),
  );
  await settle();
  return transport;
}

export async function settle() {
  for (let i = 0; i < 4; i += 1) {
    await act(async () => new Promise((resolve) => setTimeout(resolve, 0)));
    await act(async () => settled());
  }
}

export async function play(transport: FakeTransport, ...events: SetupEvent[]) {
  await act(async () => {
    for (const e of events) transport.setupEvent(e);
  });
  await settle();
}

/** A plan's block. */
export const block = (plan: "claude" | "chatgpt") =>
  document.querySelector<HTMLElement>(`[data-testid="plans-row"][data-agent="${plan}"]`);

/** The button whose words are exactly `words`, in `within` (the whole page, dialogs included, when left out). */
export const button = (words: string, within: ParentNode = document) =>
  [...within.querySelectorAll("button")].find((b) => b.textContent?.trim() === words) ?? null;

export async function press(target: HTMLElement | null) {
  if (target === null) throw new Error("nothing to press");
  await act(async () => target.click());
  await settle();
}

export async function typeInto(input: HTMLInputElement, words: string) {
  await act(async () => {
    Reflect.set(HTMLInputElement.prototype, "value", words, input);
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
}

/** The commands the window sent that are not setup commands: none, in Plans. */
export const hostCommands = (t: FakeTransport) =>
  t.only("dispatch").map(({ command }) => Object.keys(command.command)[0]);
