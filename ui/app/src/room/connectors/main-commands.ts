// The room's connector commands (U15): the only two the main window sends. The host admits `dismiss_case_law_hint` and
// `refresh_balance` from the main window and refuses every other connector command there
// (crates/desktop/src/bridge/admit.rs); the rest are the settings window's, in its own module, which nothing under
// room/ imports. The type below is the room's whole vocabulary, so there is no typed way to send anything else.
import type { HostTransport } from "../../transport/host-transport.ts";
import type { ConnectorCommand, MatterId } from "../../transport/protocol.ts";

/** What the room may send: the hint's dismissal and the balance read. */
export type MainConnectorCommand = Extract<
  ConnectorCommand,
  { dismiss_case_law_hint: unknown } | { refresh_balance: unknown }
>;

/** Sends one of the room's connector commands. A refusal goes to the console by its kind and code, never further. */
async function sendMain(transport: HostTransport, command: MainConnectorCommand): Promise<void> {
  try {
    await transport.dispatch({ command_id: crypto.randomUUID(), command: { connector: command } });
  } catch (error: unknown) {
    const code =
      typeof error === "object" && error !== null && "code" in error
        ? String((error as { code: unknown }).code)
        : "unknown";
    console.warn(`connector ${Object.keys(command)[0] ?? "connector"} failed: ${code}`);
  }
}

/** "Don't show again": the hint doesn't show in this matter again. */
export const dismissHint = (transport: HostTransport, matter: MatterId): Promise<void> =>
  sendMain(transport, { dismiss_case_law_hint: { matter } });

/** A balance read, when the main window gains focus while the out-of-credit note shows: a top-up made elsewhere then
 * clears it. The host holds reads to once a minute. */
export const refreshBalance = (transport: HostTransport, id: string): Promise<void> =>
  sendMain(transport, { refresh_balance: { id } });
