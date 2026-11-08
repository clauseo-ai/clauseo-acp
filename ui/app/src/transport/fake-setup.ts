// The setup wire, scripted for FakeTransport's `setup`. Kept apart so fake-transport.ts stays under its size.
import type { SetupCommand, SetupEvent } from "./protocol.ts";

/** The setup wire, scripted (R20's `setup` and S21's `count_ui`; store/setup.ts's SetupSource). */
export type FakeSetupHost = {
  /** The events a new listener hears first, as the host replays SetupInfo and each agent's state; none when left
   * out. */
  first?: () => readonly SetupEvent[];
  /** Answers each setup command; throwing refuses it. Its events, if any, are the test's to play (`setupEvent`). */
  command?: (command: SetupCommand) => void | Promise<void>;
};
