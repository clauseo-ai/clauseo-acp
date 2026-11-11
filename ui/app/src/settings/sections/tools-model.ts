// Settings › Tools (UC-169 on): the seam between the section and the host's MCP servers (PR A1, ui/app/src/transport/
// bindings.ts). The view is the app event `{ mcp_tools_changed: McpToolsView }`, which the host sends in every catch-up
// and after every change, and the three commands go as `HostCommand { mcp: McpCommand }` from the settings window (the
// host refuses them from the main window; only the settings window's Tools section asks for them here). Each answers
// Done, or a HostError with one of MCP_ERROR_CODES. The section reads and writes only through ToolsCommands. Secret
// values go to the host in `set` alone and never come back: a view says only which names hold a saved value. Import
// (`importable`, mcpImport) is A3's, later: no host binding has it, so the Import buttons show over a fake alone.
// Tests, the copy review and the probe hand a fake over with provideTools (tools-fake.ts, never shipped).
import { useCallback, useSyncExternalStore } from "react";
import { nextHostOf } from "../../room/host-next.ts";
import type { HostTransport } from "../../transport/host-transport.ts";
import type {
  ErrorCode,
  HostCommand,
  McpCommand,
  McpServerEdit,
  McpToolsView,
} from "../../transport/protocol.ts";

export type {
  McpControl,
  McpSecretEdit,
  McpSecretName,
  McpServerEdit,
  McpServerView,
  McpToolsView,
} from "../../transport/protocol.ts";

/** Another app whose MCP servers Clauseo can import (A3, later). */
export type ImportSource = "claudeDesktop" | "claudeCode" | "codex";

/** A source with servers to import, and how many. */
export type Importable = { source: ImportSource; count: number };

/** The section's view: the host's, and the sources found to import from (A3; only a fake sends them for now). */
export type ToolsView = McpToolsView & { importable?: Importable[] };

/** Why an import left a server out; the section says it plainly, one line a server. notApproved: a Claude Code project's
 * server the lawyer hasn't approved there. */
export type SkipReason =
  | "commandNotFound"
  | "unsupportedTransport"
  | "unresolvedVariable"
  | "alreadyAdded"
  | "unreadable"
  | "notApproved";

/** A server an import left out, and why; `program` names what commandNotFound couldn't find. */
export type Skipped = { name: string; reason: SkipReason; program?: string };

/** An import's answer: the servers added, and those left out. */
export type ImportResult = { imported: string[]; skipped: Skipped[] };

/** The host's refusals of an MCP command: A1's codes, the firm's lock (`settings.locked`) and a command the host
 * couldn't read (`command.invalid`). `mcp.command_not_found` carries no program: the form names the one it sent. */
export const MCP_ERROR_CODES = [
  "mcp.name_empty",
  "mcp.name_taken",
  "mcp.name_reserved",
  "mcp.name_invalid",
  "mcp.command_invalid",
  "mcp.command_not_found",
  "mcp.url_invalid",
  "mcp.header_name_invalid",
  "mcp.env_name_invalid",
  "mcp.secret_store_failed",
  "mcp.server_unknown",
  "settings.locked",
  "command.invalid",
] as const satisfies readonly ErrorCode[];

export type McpErrorCode = (typeof MCP_ERROR_CODES)[number];

/** A command's answer: done (null), or the host's refusal. */
export type ToolsAnswer = McpErrorCode | null;

/** What the section reads and sends. */
export interface ToolsCommands {
  /** The latest view; null until the first arrives. */
  current(): ToolsView | null;
  /** Calls `listener` with each view: the latest at once, when there is one, then each change. Answers the
   * unsubscribe. */
  subscribe(listener: (view: ToolsView) => void): () => void;
  /** `save_server`: adds a server (previousName null) or replaces one, renaming it where the name changed. */
  save(previousName: string | null, server: McpServerEdit): Promise<ToolsAnswer>;
  /** `remove_server`: removes a server and its saved values. */
  remove(name: string): Promise<ToolsAnswer>;
  /** `set_server_enabled`. */
  setEnabled(name: string, enabled: boolean): Promise<ToolsAnswer>;
  /** Imports a source's servers, their saved values with them (A3, later; a fake only for now). */
  mcpImport?(source: ImportSource): Promise<ImportResult | McpErrorCode>;
  /** Only a fake sets it: offers the remote form before REMOTE_TOOLS turns on, for its shot. */
  readonly previewRemote?: boolean;
}

/** A refusal's code as the section maps it: one of MCP_ERROR_CODES, else `command.invalid`'s generic line. */
export function refusalCode(error: unknown): McpErrorCode {
  const code =
    typeof error === "object" && error !== null && "code" in error
      ? String((error as { code: unknown }).code)
      : "";
  return (MCP_ERROR_CODES as readonly string[]).includes(code)
    ? (code as McpErrorCode)
    : "command.invalid";
}

/** The commands over a host: the view from the app feed, the commands through `HostCommand { mcp }`. Listens from the
 * first ask for the window's life, so the catch-up and every change are heard. Null where the transport has no app
 * feed. */
function hostTools(transport: HostTransport): ToolsCommands | null {
  const app = nextHostOf(transport).app;
  if (app === null) return null;
  let view: ToolsView | null = null;
  const listeners = new Set<(view: ToolsView) => void>();
  app.subscribeApp((event) => {
    if (!("mcp_tools_changed" in event)) return;
    const next = event.mcp_tools_changed;
    view = next;
    for (const listener of listeners) listener(next);
  });
  const send = async (mcp: McpCommand): Promise<ToolsAnswer> => {
    const command: HostCommand = { mcp };
    try {
      await transport.dispatch({ command_id: crypto.randomUUID(), command });
      return null;
    } catch (error: unknown) {
      return refusalCode(error);
    }
  };
  return {
    current: () => view,
    subscribe: (listener) => {
      listeners.add(listener);
      if (view !== null) listener(view);
      return () => void listeners.delete(listener);
    },
    save: (previousName, server) =>
      send({
        save_server: { ...(previousName === null ? {} : { previous_name: previousName }), server },
      }),
    remove: (name) => send({ remove_server: { name } }),
    setEnabled: (name, enabled) => send({ set_server_enabled: { name, enabled } }),
  };
}

/** Whether a lawyer may add a server at an address (PR B, remote http servers). Off until it lands; a server the
 * host already lists as remote still opens its own form. */
export const REMOTE_TOOLS: boolean = false;

/** Whether the section says the Safe workspace doesn't start a lawyer's tools, where this build has the Safe
 * workspace (sections.tsx's own signal for it, carriesProtectionWire). One line to take out. */
export const SAFE_WORKSPACE_NOTE: boolean = true;

/** Whether the form offers a server at an address. */
export const remoteOffered = (commands: ToolsCommands): boolean =>
  REMOTE_TOOLS || commands.previewRemote === true;

const provided = new WeakMap<HostTransport, ToolsCommands>();
const hosts = new WeakMap<HostTransport, ToolsCommands | null>();

/** Hands a transport its tools commands: tests, the copy review and the probe, with a fake. */
export function provideTools(transport: HostTransport, commands: ToolsCommands): void {
  provided.set(transport, commands);
}

/** The tools commands for a transport: a fake handed over, else the host's, made once and listening from the first ask; null
 * where the transport has no app feed. Only the settings window's sections ask (sections.tsx's Tools): the host accepts MCP commands from
 * the settings window alone and refuses the main window's. */
export function toolsCommandsOf(transport: HostTransport): ToolsCommands | null {
  const fake = provided.get(transport);
  if (fake !== undefined) return fake;
  if (!hosts.has(transport)) hosts.set(transport, hostTools(transport));
  return hosts.get(transport) ?? null;
}

/** Whether Tools shows: once the host has sent its first view, so a host without MCP servers shows no section. */
export const toolsShown = (transport: HostTransport): boolean =>
  toolsCommandsOf(transport)?.current() != null;

/** Whether the first view is in, as state: the settings window renders again when it arrives, so Tools appears. */
export function useToolsHeard(transport: HostTransport): boolean {
  const subscribe = useCallback(
    (wake: () => void) => toolsCommandsOf(transport)?.subscribe(() => wake()) ?? (() => {}),
    [transport],
  );
  const heard = useCallback(() => toolsShown(transport), [transport]);
  return useSyncExternalStore(subscribe, heard, heard);
}

/** Whether a command names a program by its full path: from the root (`/usr/local/bin/npx`), a drive
 * (`C:\Program Files\nodejs\npx.cmd`) or a share (`\\server\tools\run.exe`). A bare name (`npx`) is not. */
export const isFullPath = (command: string): boolean =>
  command.startsWith("/") || /^[A-Za-z]:[\\/]/.test(command) || command.startsWith("\\\\");

/** An environment value's name, as a shell takes it: letters, digits and _, not starting with a digit. */
export const ENV_NAME = /^[A-Za-z_][A-Za-z0-9_]*$/;
/** A header's name, as HTTP takes it (a token). */
export const HEADER_NAME = /^[A-Za-z0-9!#$%&'*+.^_`|~-]+$/;

/** Programs that run another one named in their arguments: the list's detail names that one instead. */
const LAUNCHERS = new Set([
  "npx",
  "uvx",
  "bunx",
  "pnpm",
  "pnpx",
  "node",
  "python",
  "python3",
  "deno",
  "docker",
]);
/** A launcher's own words before the program it runs (`docker run`, `pnpm dlx`, `deno run`). */
const SUBCOMMANDS = new Set(["run", "dlx", "exec", "x"]);

/** A local server's quiet detail: for a launcher (npx, uvx, docker, …), the first argument that isn't a flag, a
 * subcommand or a setting, by its file name; else the program's file name. */
export function localDetail(command: string, args: readonly string[]): string {
  const program = fileName(command)
    .replace(/\.(?:exe|cmd|bat)$/i, "")
    .toLowerCase();
  if (LAUNCHERS.has(program)) {
    const named = args.find((a) => !a.startsWith("-") && !SUBCOMMANDS.has(a) && !a.includes("="));
    if (named !== undefined) return fileName(named);
  }
  return fileName(command);
}

/** Whether a command is one the host takes: a bare program name (`npx`, resolved on this computer) or a full path. A
 * relative path (`bin/server`) is not (commandInvalid). */
export const isCommand = (command: string): boolean =>
  command !== "" && (isFullPath(command) || !/[\\/]/.test(command));

/** A path's last part: the quiet detail under a local server's name. */
export const fileName = (path: string): string => path.split(/[\\/]/).filter(Boolean).pop() ?? path;

/** An address's host, the quiet detail under a remote server's name; the address itself when it doesn't parse. */
export function hostOf(url: string): string {
  try {
    return new URL(url).host || url;
  } catch {
    return url;
  }
}
