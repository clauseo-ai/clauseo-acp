// R18: the main window's matter-level commands. Switching the Safe workspace off and "Use files
// from another matter" ask the host, which draws the native dialog; nothing here grants anything.
// Switching it back on needs no dialog. Forgetting a remembered answer is also the settings
// window's (settings/sections/allowances-commands.ts has its own, narrower type). Each resolves to
// the refusal's code, or null when the host took it.
import type { HostTransport } from "../../transport/host-transport.ts";
import type { AllowanceId, HostCommand, MatterId } from "../../transport/protocol.ts";

/** The matter-level commands the room sends. */
export type RoomAllowanceCommand = Extract<
  HostCommand,
  | { request_escalation: unknown }
  | { switch_protection_on: unknown }
  | { forget_allowance: unknown }
>;

/** A refusal's code; `internal` for anything that carries none. */
export const codeOf = (error: unknown): string =>
  typeof error === "object" && error !== null && "code" in error
    ? String((error as { code: unknown }).code)
    : "internal";

/** Sends one command; the refusal's code, or null. */
export async function send(
  transport: HostTransport,
  command: RoomAllowanceCommand,
): Promise<string | null> {
  try {
    await transport.dispatch({ command_id: crypto.randomUUID(), command });
    return null;
  } catch (error: unknown) {
    return codeOf(error);
  }
}

/** Asks the host to switch the matter's Safe workspace off, behind its native confirm. */
export const switchOff = (transport: HostTransport, matter: MatterId) =>
  send(transport, { request_escalation: { request: { switch_protection_off: { matter } } } });

/** Switches the matter's Safe workspace back on, from its next launch. */
export const switchOn = (transport: HostTransport, matter: MatterId) =>
  send(transport, { switch_protection_on: { matter } });

/** Asks the host to let `matter`'s agents read files of `from`: its picker, then its dialog. */
export const filesFrom = (transport: HostTransport, matter: MatterId, from: MatterId) =>
  send(transport, {
    request_escalation: { request: { files_from_other_matter: { matter, from } } },
  });

/** Forgets one of the matter's remembered answers. */
export const forget = (transport: HostTransport, matter: MatterId, allowance: AllowanceId) =>
  send(transport, { forget_allowance: { matter, allowance } });
