// Whether Plans (UD2) shows: in the settings window's first load, beside its entry, while its body loads lazily
// (plans-load.tsx).
import type { HostTransport } from "../../transport/host-transport.ts";

/** Whether the transport carries the setup wire (store/setup.ts's SetupSource): the desktop's does; a fake only when
 * its test scripts one. Plans shows only then. */
export const carriesSetup = (transport: HostTransport): boolean =>
  typeof (transport as { onSetup?: unknown }).onSetup === "function";
