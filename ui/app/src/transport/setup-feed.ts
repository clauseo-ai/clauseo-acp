// The install and sign-in events on the window's channel (R20, decision record 0048): each `{"setup": SetupEvent}`
// message `subscribe_app` brings. Desktop replays its latest `setup_info`, then each agent's install and sign-in state,
// to every new follow of the feed (a `subscribe_app`, or the feed restarted after a pause or the watchdog), and only
// then sends newer events. The window's channel follows it once, so a listener that comes later would miss that
// picture: the feed keeps the last of each and replays them to it, the way the fake does, a microtask after it
// registers. An event equal to the one kept for its slot (desktop's replay after a restart) reaches no listener.
// Out of the first load: the transport imports it on the first `onSetup` or the first setup message, and keeps the
// messages that arrive meanwhile for it (tauri-transport.ts).
//
// Setup events hold emails, URLs and device codes, so nothing here prints one: a malformed message is warned about by
// name only.
import { commands } from "./bindings.ts";
import { internalError, isHostError } from "./host-transport.ts";
import type { AgentId, HostError, SetupCommand, SetupEvent } from "./protocol.ts";

/** Sends one install or sign-in command (desktop's `setup`, `{ command }`); rejects with desktop's HostError when it
 * refuses. The command can hold a pasted code or an API key: nothing keeps or prints it, and a rejection that is not
 * desktop's own HostError (Tauri's text, which can quote what it could not decode) becomes `internal` naming the kind
 * only. */
export async function sendSetup(command: SetupCommand): Promise<void> {
  const opaque = (error: unknown): HostError =>
    isHostError(error)
      ? error
      : internalError(`setup ${Object.keys(command).join(",")} did not reach desktop`);
  let settled: Awaited<ReturnType<typeof commands.setup>>;
  try {
    settled = await commands.setup(command);
  } catch (error) {
    throw opaque(error);
  }
  if (settled.status === "error") throw opaque(settled.error);
}

type Listener = (event: SetupEvent) => void;

/** The setup event a channel message carries; null when it carries none, "malformed" when its arm is wrong. */
export function setupOf(message: object): SetupEvent | null | "malformed" {
  if (!("setup" in message)) return null;
  const event = (message as { setup: unknown }).setup;
  if (typeof event !== "object" || event === null || Array.isArray(event)) return "malformed";
  const keys = Object.keys(event);
  if (keys.length !== 1) return "malformed";
  const body = (event as Record<string, unknown>)[keys[0]!];
  if (typeof body !== "object" || body === null) return "malformed";
  const fields = body as Record<string, unknown>;
  switch (keys[0]) {
    case "setup_info":
      return typeof fields.keys_available === "boolean" ? (event as SetupEvent) : "malformed";
    case "agent_install_progress":
    case "sign_in_changed":
      return typeof fields.agent === "string" && fields.state !== undefined
        ? (event as SetupEvent)
        : "malformed";
    default:
      return "malformed";
  }
}

/** The slot an event replaces: the info, or one agent's install or sign-in. */
function slotOf(event: SetupEvent): string {
  if (event.agent_install_progress !== undefined)
    return `install:${event.agent_install_progress.agent}`;
  if (event.sign_in_changed !== undefined) return `sign_in:${event.sign_in_changed.agent}`;
  return "info";
}

export class SetupFeed {
  /** The last event of each slot, with its text, in desktop's replay order: the info first, then by agent. */
  readonly #last = new Map<string, { event: SetupEvent; text: string }>();
  readonly #agents = new Set<AgentId>();
  readonly #listeners = new Set<Listener>();
  /** Listeners waiting for their replay; they hear nothing live until it ran. */
  readonly #pending = new Set<Listener>();
  readonly #warn: (message: string) => void;

  constructor(warn: (message: string) => void) {
    this.#warn = warn;
  }

  /** One channel message with a `setup` arm: delivered, or warned about by name when malformed. */
  receive(message: object): void {
    const event = setupOf(message);
    // by name only: a setup event can hold an email, a URL or a device code
    if (event === "malformed") this.#warn("a setup message is malformed");
    else if (event !== null) this.deliver(event);
  }

  /** One event from the channel. */
  deliver(event: SetupEvent): void {
    const slot = slotOf(event);
    const text = JSON.stringify(event);
    if (this.#last.get(slot)?.text === text) return;
    this.#last.set(slot, { event, text });
    if (event.agent_install_progress !== undefined)
      this.#agents.add(event.agent_install_progress.agent);
    if (event.sign_in_changed !== undefined) this.#agents.add(event.sign_in_changed.agent);
    for (const listener of this.#listeners) this.#hand(listener, event);
  }

  /** The current picture: the info, then each agent's install and sign-in. */
  #picture(): SetupEvent[] {
    const slots = ["info"];
    for (const agent of [...this.#agents].sort())
      slots.push(`install:${agent}`, `sign_in:${agent}`);
    return slots.flatMap((slot) => {
      const last = this.#last.get(slot);
      return last === undefined ? [] : [last.event];
    });
  }

  #hand(listener: Listener, event: SetupEvent): void {
    try {
      listener(event);
    } catch (error) {
      this.#warn(`a setup listener threw: ${error instanceof Error ? error.name : typeof error}`);
    }
  }

  onSetup(listener: Listener): () => void {
    this.#pending.add(listener);
    queueMicrotask(() => {
      if (!this.#pending.delete(listener)) return;
      this.#listeners.add(listener);
      for (const event of this.#picture()) {
        if (!this.#listeners.has(listener)) return;
        this.#hand(listener, event);
      }
    });
    return () => {
      this.#pending.delete(listener);
      this.#listeners.delete(listener);
    };
  }
}
