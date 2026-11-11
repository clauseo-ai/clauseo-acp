// A fake host for Settings › Tools (tools-model.ts's ToolsCommands): its list in memory, sending the view on subscribe
// and after every change as A1's `mcp_tools_changed` does, and refusing with A1's codes. For tests, the copy review and
// the probe; never shipped. A saved value is kept apart from the view, which says only that it is saved, and `saves`
// records each save with its values replaced by "(set)", so no value reaches a test's output. Import (A3) lives here
// only for now.
import {
  ENV_NAME,
  HEADER_NAME,
  isCommand,
  isFullPath,
  type ImportResult,
  type ImportSource,
  type McpErrorCode,
  type McpSecretEdit,
  type McpServerEdit,
  type McpServerView,
  type ToolsView,
  type Skipped,
  type ToolsAnswer,
  type ToolsCommands,
} from "./tools-model.ts";

const NAME = /^[A-Za-z0-9_-]{1,64}$/;
const RESERVED = ["browser", "case-law"];
/** The bare names the fake finds on "this computer"; any other is mcp.command_not_found. */
const FOUND = ["npx", "uvx", "bunx", "node", "python3", "deno", "docker", "pnpm"];

/** What a source holds for the fake to import: servers it adds, and names it leaves out. */
export type FakeImports = Partial<Record<ImportSource, { add: McpServerView[]; skip: Skipped[] }>>;

/** A view as a fixture writes it: the sources to import come from the fake's imports. */
export type FakeView = Omit<ToolsView, "importable">;

/** One save as the fake had it, its values hidden. */
export type FakeSave = { previousName: string | null; server: McpServerEdit };

const byName = (a: McpServerView, b: McpServerView) =>
  a.name < b.name ? -1 : a.name > b.name ? 1 : 0;

const hidden = (edits: McpSecretEdit[] = []): McpSecretEdit[] =>
  edits.map((e) => ("set" in e.value ? { ...e, value: { set: "(set)" } } : e));

export class FakeTools implements ToolsCommands {
  view: FakeView;
  /** Each save, its set values as "(set)". */
  readonly saves: FakeSave[] = [];
  readonly removed: string[] = [];
  readonly toggled: { name: string; enabled: boolean }[] = [];
  readonly imported: ImportSource[] = [];
  /** The next command's refusal, whatever it carries; cleared once used. */
  refuseNext: McpErrorCode | null = null;
  readonly previewRemote: boolean;
  private readonly imports: FakeImports;
  private readonly listeners = new Set<(view: ToolsView) => void>();
  /** The values saved per server and secret name. Never shown. */
  private readonly values = new Map<string, string>();

  constructor(
    view: FakeView,
    {
      previewRemote = false,
      imports = {},
    }: { previewRemote?: boolean; imports?: FakeImports } = {},
  ) {
    this.view = { control: view.control, servers: [...view.servers] };
    this.previewRemote = previewRemote;
    this.imports = { ...imports };
  }

  /** The view as last sent: the same object until a change, as a store's snapshot must be. */
  private shown: ToolsView | null = null;

  current(): ToolsView {
    this.shown ??= this.made();
    return this.shown;
  }

  private made(): ToolsView {
    const importable = (
      Object.entries(this.imports) as [ImportSource, { add: McpServerView[]; skip: Skipped[] }][]
    )
      .map(([source, found]) => ({ source, count: found.add.length + found.skip.length }))
      .filter((i) => i.count > 0);
    return structuredClone({ ...this.view, importable });
  }

  /** Sends the view to every subscriber, as the host's `mcp_tools_changed` after a change. */
  private changed(): void {
    this.shown = null;
    for (const listener of this.listeners) listener(this.current());
  }

  subscribe(listener: (view: ToolsView) => void): () => void {
    this.listeners.add(listener);
    listener(this.current());
    return () => void this.listeners.delete(listener);
  }

  private refused(): McpErrorCode | null {
    const refusal = this.refuseNext;
    this.refuseNext = null;
    if (refusal !== null) return refusal;
    return this.view.control === "lawyer" ? null : "settings.locked";
  }

  mcpImport(source: ImportSource): Promise<ImportResult | McpErrorCode> {
    const refusal = this.refused();
    if (refusal !== null) return Promise.resolve(refusal);
    const found = this.imports[source] ?? { add: [], skip: [] };
    delete this.imports[source];
    this.imported.push(source);
    const taken = new Set(this.view.servers.map((s) => s.name));
    const add = found.add.filter((s) => !taken.has(s.name));
    this.view.servers = [...this.view.servers, ...add].sort(byName);
    this.changed();
    return Promise.resolve({
      imported: add.map((s) => s.name),
      skipped: [
        ...found.skip,
        ...found.add
          .filter((s) => taken.has(s.name))
          .map((s) => ({ name: s.name, reason: "alreadyAdded" as const })),
      ],
    });
  }

  save(previousName: string | null, server: McpServerEdit): Promise<ToolsAnswer> {
    this.saves.push({
      previousName,
      server: {
        ...server,
        ...(server.env === undefined ? {} : { env: hidden(server.env) }),
        ...(server.headers === undefined ? {} : { headers: hidden(server.headers) }),
      },
    });
    const answer = this.check(previousName, server);
    if (answer === null) this.changed();
    return Promise.resolve(answer);
  }

  private check(previousName: string | null, server: McpServerEdit): ToolsAnswer {
    const refusal = this.refused();
    if (refusal !== null) return refusal;
    const name = server.name.trim();
    if (name === "") return "mcp.name_empty";
    if (!NAME.test(name) || name.includes("__") || name.endsWith("_")) return "mcp.name_invalid";
    if (RESERVED.includes(name)) return "mcp.name_reserved";
    if (name !== previousName && this.view.servers.some((s) => s.name === name))
      return "mcp.name_taken";
    if (previousName !== null && !this.view.servers.some((s) => s.name === previousName))
      return "mcp.server_unknown";
    const command = server.command ?? "";
    if (server.kind === "local" && !isCommand(command)) return "mcp.command_invalid";
    if (server.kind === "local" && !isFullPath(command) && !FOUND.includes(command))
      return "mcp.command_not_found";
    if (server.kind === "remote" && !/^https?:\/\/[^\s/]+/.test(server.url ?? ""))
      return "mcp.url_invalid";
    const env = server.env ?? [];
    const headers = server.headers ?? [];
    if (env.some((e) => !ENV_NAME.test(e.name))) return "mcp.env_name_invalid";
    if (headers.some((h) => !HEADER_NAME.test(h.name))) return "mcp.header_name_invalid";
    const before = this.view.servers.find((s) => s.name === previousName);
    // the values this save leaves: each set one, and each kept one from before; the rest are deleted
    const kept = new Map<string, string>();
    const secrets = (edits: McpSecretEdit[], kind: string) =>
      edits.map((e) => {
        const was = this.values.get(`${previousName ?? ""} ${kind} ${e.name}`);
        const value = "set" in e.value ? e.value.set : was;
        if (value !== undefined) kept.set(`${name} ${kind} ${e.name}`, value);
        return { name: e.name, saved: value !== undefined };
      });
    const view: McpServerView = {
      name,
      kind: server.kind,
      ...(server.kind === "local" ? { command } : { url: server.url ?? "" }),
      args: server.kind === "local" ? (server.args ?? []) : [],
      env: server.kind === "local" ? secrets(env, "env") : [],
      headers: server.kind === "remote" ? secrets(headers, "header") : [],
      enabled: before?.enabled ?? server.enabled,
    };
    for (const key of this.values.keys())
      if (key.startsWith(`${previousName ?? name} `) || key.startsWith(`${name} `))
        this.values.delete(key);
    for (const [key, value] of kept) this.values.set(key, value);
    this.view.servers = [
      ...this.view.servers.filter((s) => s.name !== previousName && s.name !== name),
      view,
    ].sort(byName);
    return null;
  }

  remove(name: string): Promise<ToolsAnswer> {
    const refusal = this.refused();
    if (refusal !== null) return Promise.resolve(refusal);
    this.removed.push(name);
    this.view.servers = this.view.servers.filter((s) => s.name !== name);
    this.changed();
    return Promise.resolve(null);
  }

  setEnabled(name: string, enabled: boolean): Promise<ToolsAnswer> {
    const refusal = this.refused();
    if (refusal !== null) return Promise.resolve(refusal);
    this.toggled.push({ name, enabled });
    this.view.servers = this.view.servers.map((s) => (s.name === name ? { ...s, enabled } : s));
    this.changed();
    return Promise.resolve(null);
  }

  /** Whether a value is saved under a server's secret name, for a test to check a kept value survived. */
  holds(server: string, kind: "env" | "header", name: string): boolean {
    return this.values.has(`${server} ${kind} ${name}`);
  }

  /** Seeds a saved value, as an earlier save would have left it. */
  seed(server: string, kind: "env" | "header", name: string): void {
    this.values.set(`${server} ${kind} ${name}`, "seeded");
  }
}

/** A local server for fixtures. */
export const local = (
  name: string,
  command: string,
  over: Partial<McpServerView> = {},
): McpServerView => ({
  name,
  kind: "local",
  command,
  args: [],
  env: [],
  headers: [],
  enabled: true,
  ...over,
});
