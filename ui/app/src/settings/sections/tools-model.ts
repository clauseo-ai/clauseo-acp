// Settings › Tools (UC-169 on): the seam between the section and the host's MCP server protocol (PR A1). The types
// below are A1's names as its writer gave them (not committed yet): the view arrives as the app event
// `{ mcp_tools_changed: McpToolsView }`, in every catch-up and after every change, and the commands go as
// `HostCommand { mcp: McpCommand }`, each answering Done or a HostError with one of MCP_ERROR_CODES. The section reads
// and writes only through ToolsCommands, which this file finds for a transport. Until A1's bindings land no transport
// carries them, so toolsCommandsOf answers null and the section stays hidden; tests, the copy review and the probe hand
// a fake over with provideTools (tools-fake.ts). When A1 lands, its bindings are wired here and nowhere else. Secret
// values go to the host in `set` alone and never come back: a view says only which names hold a saved value. Import
// (`importable`, mcpImport) is A3's, later, and only the fake has it for now.
import type { HostTransport } from "../../transport/host-transport.ts";

/** Who holds the list: the lawyer (editable); the firm's policy `mcp.servers` (read-only, locked); or the firm's
 * managed-mcp.json, which runs MCP itself (no list to show). */
export type McpControl = "lawyer" | "firm_list" | "firm_managed";

/** A secret's name, and whether a value is saved for it. The value never leaves the host. */
export type McpSecretName = { name: string; saved: boolean };

/** One server, as the host shows it. */
export type McpServerView = {
  name: string;
  kind: "local" | "remote";
  /** Local: what the lawyer typed, a bare program name or a full path. */
  command?: string;
  /** Local: its arguments, in order. */
  args: string[];
  /** Remote: its address. */
  url?: string;
  env: McpSecretName[];
  /** Remote: its headers. */
  headers: McpSecretName[];
  enabled: boolean;
};

/** Another app whose MCP servers Clauseo can import (A3, later). */
export type ImportSource = "claudeDesktop" | "claudeCode" | "codex";

/** A source with servers to import, and how many. */
export type Importable = { source: ImportSource; count: number };

/** The section's whole view: who holds the list and the servers in name order; `importable` (A3, the fake only for
 * now) the sources found to import from. */
export type McpToolsView = {
  control: McpControl;
  servers: McpServerView[];
  importable?: Importable[];
};

/** Why an import left a server out; the section says it plainly, one line a server. notApproved: a Claude Code project's
 * server the lawyer hasn't approved there. */
export type SkipReason =
  | "commandNotFound"
  | "unsupportedTransport"
  | "unresolvedVariable"
  | "alreadyAdded"
  | "unreadable"
  | "notApproved";

/** An import's answer: the servers added, and those left out. */
export type ImportResult = { imported: string[]; skipped: Skipped[] };

/** A server an import left out, and why; `program` names what commandNotFound couldn't find. */
export type Skipped = { name: string; reason: SkipReason; program?: string };

/** A secret in a save: keep the value saved under this name, or set a new one. A name saved before and missing now
 * is removed, its saved value deleted. */
export type McpSecretEdit = {
  name: string;
  value: { keep: Record<string, never> } | { set: string };
};

/** A server as a save sends it (`McpCommand::save_server`'s `server`). */
export type McpServerEdit = {
  name: string;
  kind: "local" | "remote";
  command?: string;
  args: string[];
  url?: string;
  env: McpSecretEdit[];
  headers: McpSecretEdit[];
  enabled: boolean;
};

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
] as const;

export type McpErrorCode = (typeof MCP_ERROR_CODES)[number];

/** A command's answer: done (null), or the host's refusal. */
export type ToolsAnswer = McpErrorCode | null;

/** What the section reads and sends. */
export interface ToolsCommands {
  /** Calls `listener` with the view now (the catch-up) and after every change; answers the unsubscribe. */
  subscribe(listener: (view: McpToolsView) => void): () => void;
  /** `save_server`: adds a server (previousName null) or replaces one, renaming it where the name changed. */
  save(previousName: string | null, server: McpServerEdit): Promise<ToolsAnswer>;
  /** `remove_server`: removes a server and its saved values. */
  remove(name: string): Promise<ToolsAnswer>;
  /** `set_server_enabled`. */
  setEnabled(name: string, enabled: boolean): Promise<ToolsAnswer>;
  /** Imports a source's servers, their saved values with them (A3, later; the fake only for now). */
  mcpImport?(source: ImportSource): Promise<ImportResult | McpErrorCode>;
  /** Only a fake sets it: offers the remote form before REMOTE_TOOLS turns on, for its shot. */
  readonly previewRemote?: boolean;
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

/** Hands a transport its tools commands: tests, the copy review and the probe, with a fake. */
export function provideTools(transport: HostTransport, commands: ToolsCommands): void {
  provided.set(transport, commands);
}

/** The tools commands a transport carries, or null where it carries none (the section hides). A1 wires the host's
 * bindings in here. Only the settings window's sections ask (sections.tsx's Tools): the host accepts MCP commands from
 * the settings window alone and refuses the main window's. */
export const toolsCommandsOf = (transport: HostTransport): ToolsCommands | null =>
  provided.get(transport) ?? null;

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
