// The settings window's connector commands (U15): the only place the app sends them. The host admits these from the
// settings window alone (crates/desktop/src/bridge/admit.rs); the room sends only `dismiss_case_law_hint` and
// `refresh_balance`, from its own module. A command goes out once through the transport and nothing here keeps it:
// a pasted key is in the one envelope `add_api_key` dispatches and in no store, no log and no state after.
import type { HostTransport } from "../transport/host-transport.ts";
import type { ConnectorCommand } from "../transport/protocol.ts";

/** What Settings may send: every connector command but the room's hint dismissal. */
export type SettingsConnectorCommand = Exclude<
  ConnectorCommand,
  { dismiss_case_law_hint: unknown }
>;

/** Sends one connector command. Resolves once the host has taken it; rejects with the HostError when it refuses. The
 * rejection names the command's kind, never its fields, so a key can't reach the console. */
export async function sendConnector(
  transport: HostTransport,
  command: SettingsConnectorCommand,
): Promise<void> {
  try {
    await transport.dispatch({ command_id: crypto.randomUUID(), command: { connector: command } });
  } catch (error: unknown) {
    const kind = Object.keys(command)[0] ?? "connector";
    const code =
      typeof error === "object" && error !== null && "code" in error
        ? String((error as { code: unknown }).code)
        : "unknown";
    console.warn(`connector ${kind} failed: ${code}`);
    throw error;
  }
}
