// The list from stage D's list_addons (D1): read when the source is made (at the room's start), again
// on each `addons_changed` app event and each time the place opens; each read also checks Clauseo
// Case Law's reach again on the host when it was out of reach. An added tool's server comes on its
// row (D2a); a row without one takes Settings' view (`mcp_tools_changed`) by name. Case Law is
// listed even before the host has read its credential once (the host leaves it out until then):
// "needs_setup", as the host words "not yet checked".
import { nextHostOf } from "../room/host-next.ts";
import { CASE_LAW, connectorsStoreOf } from "../store/connectors.ts";
import type { HostTransport } from "../transport/host-transport.ts";
import type { McpServerView } from "../transport/protocol.ts";
import type { AddonView } from "./addons.ts";
import type { ListReader, ListState } from "./addons-source.ts";

/** Case Law's row while the host lists none yet: not yet checked. */
const NOT_CHECKED: AddonView = {
  kind: "tool",
  id: CASE_LAW,
  name: "Case law (Indian Kanoon)",
  description: "",
  source: "built_in",
  scope: "all",
  on: true,
  status: "needs_setup",
  can_remove: false,
  locked: false,
};

/** The host's rows as the place lists them: Case Law at the head of the tools when the host left it
 * out, and an added tool without a server on its row given Settings' view of it when it is there. */
export function listed(
  items: readonly AddonView[],
  servers: readonly McpServerView[],
): AddonView[] {
  const rows = items.map((a) => {
    if (a.kind !== "tool" || a.source !== "added" || a.server !== undefined) return a;
    const s = servers.find((v) => v.name === a.id);
    return s === undefined ? a : { ...a, server: s };
  });
  if (rows.some((a) => a.id === CASE_LAW && a.source === "built_in")) return rows;
  const at = rows.findIndex((a) => a.kind === "tool");
  rows.splice(at < 0 ? rows.length : at, 0, NOT_CHECKED);
  return rows;
}

/** The reader: complete once list_addons has answered and the connectors' state has been heard (the
 * rail's count waits for that); without an app feed, once the list has answered. */
export function addonsList(transport: HostTransport): ListReader {
  const next = nextHostOf(transport);
  const connectors = connectorsStoreOf(transport);
  const feedless = next.app == null;
  let items: readonly AddonView[] | null = null;
  let servers: readonly McpServerView[] = [];
  let state: ListState = { items: null, complete: false };
  let asked = 0;
  const listeners = new Set<() => void>();
  const compose = () => {
    if (items === null) return;
    state = { items: listed(items, servers), complete: feedless || connectors.heard() };
    for (const listener of listeners) listener();
  };
  const read = async () => {
    const n = (asked += 1);
    try {
      const answer = await transport.query({ list_addons: {} });
      if (n === asked) items = answer.addons?.items ?? [];
    } catch {
      // the list stays as last read; a first read that fails lists Case Law alone, not yet checked
      if (n === asked && items === null) items = [];
    }
    if (n === asked) compose();
  };
  next.app?.subscribeApp((event) => {
    if ("addons_changed" in event) void read();
    else if ("mcp_tools_changed" in event) {
      servers = event.mcp_tools_changed.servers;
      compose();
    }
  });
  connectors.subscribe(compose);
  void read();
  return {
    subscribe: (listener) => {
      listeners.add(listener);
      return () => void listeners.delete(listener);
    },
    state: () => state,
    read: () => void read(),
  };
}
