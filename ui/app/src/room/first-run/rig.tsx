// Shared setup for the first run's tests (UD1): the room over a FakeTransport that carries the setup wire, with a
// matter open or none (the first screen), the host's privacy settings, and helpers to play SetupEvents and read what
// the window sent. Not shipped: only tests import it.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { settled } from "@clauseo/components/lib/late.ts";
import { afterEach, vi } from "vitest";
import type { Lang } from "@clauseo/components/lib/display.ts";
import { uuid } from "../../testing/stream-script.ts";
import { FakeTransport, type FakeHost } from "../../transport/fake-transport.ts";
import type {
  AgentId,
  CommandOutcome,
  HostCommand,
  InstallProgress,
  MatterSummary,
  SettingsView,
  SetupCommand,
  SetupEvent,
  SignIn,
  UiCount,
} from "../../transport/protocol.ts";
import { Room } from "../room.tsx";
import { connectRoom } from "../start.ts";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

export const A: MatterSummary = { matter: uuid(1, 2), name: "Acme Ltd v. State", kind: "folder" };
export const MADE_MATTER = uuid(5, 2);
export const MADE_THREAD = uuid(9, 1);
export const CLAUDE = "claude-acp";
export const CODEX = "codex-acp";
export const AT = Date.UTC(2026, 10, 5, 11, 2);

let host: HTMLDivElement | null = null;
let root: Root | null = null;

afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  root = null;
  host = null;
  vi.useRealTimers();
  vi.restoreAllMocks();
  Reflect.deleteProperty(navigator, "clipboard");
});

/** A clipboard to copy into, as the webview has: its writeText, to read what was copied. */
export function stubClipboard() {
  const writeText = vi.fn(async (_text: string) => {});
  Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
  return writeText;
}

/** The host's privacy settings, before the first act unless `started`. */
export function settingsView(
  over: { counts?: boolean; started?: boolean; locked?: string[] } = {},
): SettingsView {
  return {
    ui: { theme: "system", font_size: 16, language: "en", lite: "automatic", motion: "system" },
    tools: {} as SettingsView["tools"],
    privacy: {
      counts: over.counts ?? true,
      crash_reports: false,
      counts_started: over.started ?? false,
    },
    locked: over.locked ?? [],
  } as SettingsView;
}

export const install = (agent: AgentId, state: InstallProgress): SetupEvent => ({
  agent_install_progress: { agent, state },
});
export const signIn = (agent: AgentId, state: SignIn): SetupEvent => ({
  sign_in_changed: { agent, state },
});
export const info = (keys = true): SetupEvent => ({ setup_info: { keys_available: keys } });
export const SIGNED_IN: SignIn = {
  signed_in: {
    email: "test@example.com",
    plan: "Claude Max",
    method: "plan",
    source: "own",
    at: AT,
  },
};
export const READY: InstallProgress = { ready: { version: "0.1.0" } };

export type RigOptions = {
  /** The matters the host lists; none shows the first screen. */
  matters?: MatterSummary[];
  /** The events a new setup listener hears first. */
  first?: SetupEvent[];
  settings?: SettingsView;
  platform?: "mac" | "windows" | "linux";
  language?: Lang;
  /** The folder the picker gives; null: cancelled. */
  picked?: { folder: string; display: string } | null;
};

/** The room over a fake host with the setup wire. */
export async function mountFirstRun(o: RigOptions = {}) {
  let matters = o.matters ?? [A];
  const fake: FakeHost = {
    platform: o.platform ?? "mac",
    query: async (query) => {
      if (query === "list_matters") return { matters };
      if (query === "get_settings") return { settings: o.settings ?? settingsView() };
      if (typeof query === "object" && "list_threads" in query) return { threads: [] };
      throw new Error(`not scripted: ${JSON.stringify(query)}`);
    },
    dispatch: (envelope): CommandOutcome => {
      const c = envelope.command;
      if (c.create_thread !== undefined) return { thread_created: { thread: MADE_THREAD } };
      if (c.create_matter !== undefined) {
        matters = [{ matter: MADE_MATTER, name: c.create_matter.name, kind: "folder" }];
        return { matter_created: { matter: MADE_MATTER } };
      }
      return "done";
    },
    pickFolder: () =>
      o.picked === undefined
        ? { folder: "folder-ref-1", display: "Documents/Matters/Rathi Steels" }
        : o.picked,
    setup: { first: () => o.first ?? [] },
  };
  const transport = new FakeTransport(fake);
  connectRoom(transport, { language: o.language ?? "en" });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root?.render(<Room transport={transport} />));
  await settle();
  return transport;
}

/** Lets the answers, the lazy chunks and the effects run. */
export async function settle() {
  for (let i = 0; i < 4; i += 1) {
    await act(async () => new Promise((resolve) => setTimeout(resolve, 0)));
    await act(async () => settled());
  }
}

/** Plays SetupEvents as the host's stream would, then lets the room render. */
export async function play(transport: FakeTransport, ...events: SetupEvent[]) {
  await act(async () => {
    for (const e of events) transport.setupEvent(e);
  });
  await settle();
}

export const byId = <E extends HTMLElement = HTMLElement>(id: string) =>
  document.querySelector<E>(`[data-testid="${id}"]`);
export const allById = (id: string) => [...document.querySelectorAll(`[data-testid="${id}"]`)];
export const text = () => host?.textContent ?? "";

/** The button whose words are exactly `words`, in the room. */
export const button = (words: string) =>
  [...(host?.querySelectorAll("button") ?? [])].find((b) => b.textContent?.trim() === words) ??
  null;

export async function press(target: HTMLElement | null | string) {
  const el = typeof target === "string" ? button(target) : target;
  if (el === null)
    throw new Error(`nothing to press: ${typeof target === "string" ? target : "no element"}`);
  await act(async () => el.click());
  await settle();
}

/** Types into an input as the lawyer would. */
export async function typeInto(input: HTMLInputElement | HTMLTextAreaElement, words: string) {
  const proto = input instanceof HTMLTextAreaElement ? HTMLTextAreaElement : HTMLInputElement;
  await act(async () => {
    Reflect.set(proto.prototype, "value", words, input);
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
}

/** Presses Enter in a field. */
export async function enter(field: HTMLElement) {
  await act(async () =>
    field.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true }),
    ),
  );
  await settle();
}

/** The setup commands sent. */
export const setups = (t: FakeTransport): SetupCommand[] => t.setups();
/** The counts sent. */
export const counts = (t: FakeTransport): UiCount[] => t.only("count_ui").map((c) => c.event);
/** The host commands dispatched, of one kind. */
export const sent = <K extends keyof HostCommand>(t: FakeTransport, kind: K) =>
  t
    .only("dispatch")
    .map((c) => c.command.command[kind])
    .filter((c) => c !== undefined) as NonNullable<HostCommand[K]>[];
