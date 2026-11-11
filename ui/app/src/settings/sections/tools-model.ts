// Settings › Tools (UC-169 to UC-189): the seam between the section and the host's MCP server protocol (plan.md's
// contract, PR A1). The types below are the contract's shape; the section reads and writes only through ToolsCommands,
// which this file finds for a transport. Until A1's bindings land no transport carries them, so toolsCommandsOf answers
// null and the section stays hidden; tests, the copy review and the probe hand a fake over with provideTools
// (tools-fake.ts). When A1 lands, its bindings are wired here and nowhere else. Secret values go to the host in
// `set` alone and never come back: a view says only which names hold a saved value.
import type { HostTransport } from "../../transport/host-transport.ts";

/** Who holds the list: the lawyer (editable); the firm's policy `mcp.servers` (read-only, locked); or the firm's
 * managed-mcp.json, which runs MCP itself (no list to show). */
export type McpControl = "lawyer" | "firmList" | "firmManaged";

/** A secret's name, and whether a value is saved for it. The value never leaves the host. */
export type SecretName = { name: string; saved: boolean };

/** One server, as the host shows it. */
export type McpServerView = {
  name: string;
  kind: "local" | "remote";
  /** Local: the program's absolute path. */
  command: string | null;
  /** Local: its arguments, in order. */
  args: string[];
  /** Remote: its address. */
  url: string | null;
  env: SecretName[];
  /** Remote: its headers. */
  headers: SecretName[];
  enabled: boolean;
};

/** The section's whole view: who holds the list, and the servers in name order. */
export type McpToolsView = { control: McpControl; servers: McpServerView[] };

/** A secret in a save: keep the value saved under this name, or set a new one. A name saved before and missing now
 * is removed, its saved value deleted. */
export type SecretEdit = { name: string; value: { keep: true } | { set: string } };

/** A server as the form saves it. */
export type McpServerDraft = {
  name: string;
  kind: "local" | "remote";
  command: string | null;
  args: string[];
  url: string | null;
  env: SecretEdit[];
  headers: SecretEdit[];
  enabled: boolean;
};

/** What the host refuses a command with. */
export type McpToolsError =
  | "nameEmpty"
  | "nameTaken"
  | "nameReserved"
  | "nameInvalid"
  | "commandNotAbsolute"
  | "urlInvalid"
  | "headerNameInvalid"
  | "envNameInvalid"
  | "lockedByFirm"
  | "secretStoreFailed";

/** A command's answer: done (null), or the host's refusal. */
export type ToolsAnswer = McpToolsError | null;

/** What the section reads and sends. */
export interface ToolsCommands {
  /** The view as the host has it now. */
  read(): Promise<McpToolsView>;
  /** Adds a server (previousName null) or replaces one, renaming it where the name changed. */
  save(previousName: string | null, server: McpServerDraft): Promise<ToolsAnswer>;
  /** Removes a server and its saved values. */
  remove(name: string): Promise<ToolsAnswer>;
  setEnabled(name: string, enabled: boolean): Promise<ToolsAnswer>;
  /** Only a fake sets it: offers the remote form before REMOTE_TOOLS turns on, for its shot. */
  readonly previewRemote?: boolean;
}

/** Whether a lawyer may add a server at an address (PR B, remote http servers). Off until it lands; a server the
 * host already lists as remote still opens its own form. */
export const REMOTE_TOOLS: boolean = false;

/** Whether the section says the Safe workspace doesn't start a lawyer's tools (plan.md's open question; the default
 * until it's answered). One line to take out. */
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
 * bindings in here. */
export const toolsCommandsOf = (transport: HostTransport): ToolsCommands | null =>
  provided.get(transport) ?? null;

/** Whether a command names a program by its full path: from the root (`/usr/local/bin/npx`), a drive
 * (`C:\Program Files\nodejs\npx.cmd`) or a share (`\\server\tools\run.exe`). A bare name (`npx`) is not. */
export const isFullPath = (command: string): boolean =>
  command.startsWith("/") || /^[A-Za-z]:[\\/]/.test(command) || command.startsWith("\\\\");

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
