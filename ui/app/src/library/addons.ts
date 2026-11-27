// Stage D's skills and connectors list, as the app reads it: the host's own types (bindings.ts) for
// the answer to {"list_addons": {"matter"?: id}} and the event {"addons_changed": {}}, with D2a's
// switched matters and an added tool's server on each row, and the icon D3/D5 add. Every reader
// imports from here, and every change is sent as a command from here (`toCommand`).
import type {
  Addon,
  HostCommand,
  MatterId,
  SetAddonOn,
  SetAddonScope,
} from "../transport/bindings.ts";

export type { AddonFromKind, AddonList, AddonScope, AddonStatus } from "../transport/bindings.ts";

/** One item of list_addons, with what later stages add. */
export type AddonView = Addon & {
  /** D3/D5: the host-fetched site icon or GitHub avatar, as a data: URL. The app never
   * fetches. */
  icon?: string;
  /** The app's own: `on` as list_addons {matter} answers it for the matter open last, which
   * says what the matter gets; left out until read, and on a row held while a change is with
   * the host. */
  here_on?: { matter: MatterId; on: boolean };
};

/** What one change asks of the host. */
export type AddonChange =
  | { set_addon_on: SetAddonOn }
  | { set_addon_scope: SetAddonScope }
  | { remove_addon: { kind: AddonView["kind"]; id: string } };

/** A change as the host's command. */
export const toCommand = (change: AddonChange): HostCommand => change;
