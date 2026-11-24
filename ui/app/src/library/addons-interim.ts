// The list before stage D's list_addons exists (D1), made from what the main window can read today:
// the built-ins the app knows, the skill packs (`list_skill_packs`, read at the start, on each
// `settings_changed` and when the place opens), the MCP servers (`mcp_tools_changed`, in every app
// catch-up), and Clauseo Case Law from the connectors' store (`connectors_changed`). Every item is
// scope "all" here, as D1's are. Removed with the switch to list_addons (addons-source.ts).
import { nextHostOf } from "../room/host-next.ts";
import { CASE_LAW, connectorsStoreOf } from "../store/connectors.ts";
import type { HostTransport } from "../transport/host-transport.ts";
import type {
  ConnectorsState,
  McpServerView,
  McpToolsView,
  PackStatus,
} from "../transport/protocol.ts";
import type { AddonStatus, AddonView } from "./addons.ts";
import type { ListReader, ListState } from "./addons-source.ts";

const builtIn = (kind: AddonView["kind"], id: string, name: string): AddonView => ({
  kind,
  id,
  name,
  source: "built_in",
  description: "",
  scope: "all",
  on: true,
  status: "ready",
  can_remove: false,
  locked: false,
});

/** The built-in skills, in the place's order; agent-browser is left out (the Web browser's detail
 * names it). */
const SKILLS: readonly AddonView[] = [
  builtIn("skill", "legal-drafting", "Legal drafting"),
  builtIn("skill", "legal-research", "Legal research"),
  builtIn("skill", "matter-brief", "Matter brief"),
  builtIn("skill", "stetdoc", "Word documents"),
];

const BROWSER = builtIn("tool", "browser", "Web browser");

const hostOf = (url: string): string => {
  try {
    return new URL(url).host;
  } catch {
    return url;
  }
};

const PACK_STATUS: Record<PackStatus["state"], AddonStatus> = {
  on: "ready",
  off: "ready",
  changed: "changed",
  invalid: "invalid",
  not_admitted: "blocked",
};

/** A skill pack as an added skill: a folder when added on this computer, else its source address.
 */
export function packItem(pack: PackStatus): AddonView {
  const url = pack.source_url;
  const from: AddonView["from"] =
    pack.added_here === true
      ? { kind: "folder", text: pack.id }
      : url === undefined
        ? undefined
        : hostOf(url) === "github.com"
          ? { kind: "github", text: new URL(url).pathname.replace(/^\//, "") }
          : { kind: "url", text: url };
  return {
    kind: "skill",
    id: pack.id,
    name: pack.name ?? pack.id,
    source: "added",
    description: "",
    ...(from === undefined ? {} : { from }),
    scope: "all",
    on: pack.state === "on",
    ...(pack.instruction_only === undefined ? {} : { has_scripts: !pack.instruction_only }),
    status: PACK_STATUS[pack.state],
    can_remove: pack.added_here === true,
    locked: false,
  };
}

/** An MCP server as one of the lawyer's own connectors. */
export function serverItem(server: McpServerView, locked: boolean): AddonView {
  const secrets = [...server.env, ...server.headers];
  const from: AddonView["from"] =
    server.kind === "remote"
      ? { kind: "address", text: server.url ?? "" }
      : { kind: "program", text: server.command ?? "" };
  return {
    kind: "tool",
    id: server.name,
    name: server.name,
    source: "added",
    description: "",
    from,
    server: {
      ...(server.command === undefined ? {} : { command: server.command }),
      args: server.args,
      ...(server.url === undefined ? {} : { url: server.url }),
      env: server.env.map((s) => ({ name: s.name, saved: s.saved })),
      headers: server.headers.map((s) => ({ name: s.name, saved: s.saved })),
      enabled: server.enabled,
    },
    scope: "all",
    on: server.enabled,
    status: secrets.some((s) => !s.saved) ? "needs_secret" : "ready",
    can_remove: !locked,
    locked,
  };
}

/** Clauseo Case Law's status from the connectors' state. The host lists it while live or added, so
 * a state heard without it means the liveness check failed. */
export function caseLawStatus(state: ConnectorsState, heard: boolean): AddonStatus {
  if (!heard) return "needs_setup";
  if (!state.available) return state.locked ? "blocked" : "needs_setup";
  const c = state.connectors.find((v) => v.id === CASE_LAW);
  if (c === undefined) return "unreachable";
  if (typeof c.status === "object" && ("signed_in" in c.status || "out_of_credit" in c.status))
    return "ready";
  return "needs_setup";
}

/** The whole list, in the place's order. */
export function interimItems(
  packs: readonly PackStatus[],
  tools: McpToolsView | null,
  connectors: ConnectorsState,
  heard: boolean,
): AddonView[] {
  const status = caseLawStatus(connectors, heard);
  const caseLaw = { ...builtIn("tool", CASE_LAW, "Clauseo Case Law"), status };
  const locked = tools !== null && tools.control !== "lawyer";
  return [
    ...SKILLS,
    ...packs.map(packItem),
    caseLaw,
    BROWSER,
    ...(tools?.servers ?? []).map((s) => serverItem(s, locked)),
  ];
}

/** The list over today's reads: complete once the packs, the MCP servers and the connectors' state
 * have each been heard (the rail's count waits for that). */
export function interimList(transport: HostTransport): ListReader {
  const next = nextHostOf(transport);
  const connectors = connectorsStoreOf(transport);
  let packs: readonly PackStatus[] | null = null;
  let tools: McpToolsView | null = null;
  let state: ListState = { items: null, complete: false };
  let asked = 0;
  const listeners = new Set<() => void>();
  const compose = () => {
    if (packs === null) return;
    const heard = connectors.heard();
    state = {
      items: interimItems(packs, tools, connectors.state(), heard),
      complete: tools !== null && heard,
    };
    for (const listener of listeners) listener();
  };
  const read = async () => {
    const n = (asked += 1);
    try {
      const answer = await transport.query("list_skill_packs");
      if (n === asked) packs = answer.skill_packs ?? [];
    } catch {
      // the list stays as last read; a first read that fails lists no added skill
      if (n === asked && packs === null) packs = [];
    }
    if (n === asked) compose();
  };
  next.app?.subscribeApp((event) => {
    if (!("mcp_tools_changed" in event)) return;
    tools = event.mcp_tools_changed;
    compose();
  });
  connectors.subscribe(compose);
  next.settings?.onSettingsChanged(() => void read());
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
