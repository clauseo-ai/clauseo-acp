// The setup commands the room's expired sign-in card may send (UD2's window split): the six sign-in ones, and nothing of
// Plans' (sign out, a key's removal, the firm's address, finding or leaving a login), which desktop refuses from the
// main window anyway (crates/desktop/src/setup/admit.rs). Typed over those six, as U15's main-commands.ts is, so the
// room has no typed way to send a Plans command.
import type { SetupStore } from "../../store/setup.ts";
import type { SetupCommand } from "../../transport/protocol.ts";

/** The six the card sends. */
export type RoomSetupCommand = Extract<
  SetupCommand,
  | { start_sign_in: unknown }
  | { use_different_account: unknown }
  | { cancel_sign_in: unknown }
  | { paste_sign_in_code: unknown }
  | { use_device_code: unknown }
  | { open_sign_in_page: unknown }
>;

/** Sends one of the six; a refusal goes to the agent's `refused` (store/setup.ts). */
export function roomCommand(store: SetupStore, command: RoomSetupCommand): void {
  store.command(command);
}

/** One of the six, with whether desktop took it: the paste field keeps its code until then. */
export function roomSend(store: SetupStore, command: RoomSetupCommand): Promise<boolean> {
  return store.send(command);
}
