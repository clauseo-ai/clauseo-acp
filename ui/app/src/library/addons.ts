// Stage D's skills and connectors list, as the app reads it: the host's own types (bindings.ts) for
// the answer to {"list_addons": {"matter"?: id}} and the event {"addons_changed": {}}, and the
// fields D2a adds (scope's switched matters, an added tool's server, an icon), which the app reads
// from the older queries until then (addons-list.ts). Every reader imports from here.
import type { Addon } from "../transport/bindings.ts";
import type { MatterId } from "../transport/protocol.ts";

export type { AddonFromKind, AddonList, AddonScope, AddonStatus } from "../transport/bindings.ts";

/** One item of list_addons, with what D2a and later add. */
export type AddonView = Addon & {
  /** D2a: the matters whose switch differs from the scope's default. */
  switched?: MatterId[];
  /** D2a: an added tool's server as today's McpServerView has it, names only; read from
   * `mcp_tools_changed` until then. */
  server?: AddonServer;
  /** D3/D5: the host-fetched site icon or GitHub avatar, as a data: URL. The app never fetches. */
  icon?: string;
};

/** A saved value's name, and whether a value is held; never the value. */
export type AddonSecretName = { name: string; saved: boolean };

/** An added tool's server: its program and arguments, or its address. */
export type AddonServer = {
  command?: string;
  args: string[];
  url?: string;
  env: AddonSecretName[];
  headers: AddonSecretName[];
  enabled: boolean;
};
