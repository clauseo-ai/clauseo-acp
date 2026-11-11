// A fake host for Settings › Tools (tools-model.ts's ToolsCommands): its list in memory, refusing as plan.md's contract
// says the host does. For tests, the copy review and the probe; never shipped. A saved value is kept apart from the
// view, which says only that it is saved, and `saves` records each save with its values replaced by "(set)", so no
// value reaches a test's output.
import {
  isFullPath,
  type McpServerDraft,
  type McpServerView,
  type McpToolsError,
  type McpToolsView,
  type ToolsAnswer,
  type ToolsCommands,
} from "./tools-model.ts";

const NAME = /^[A-Za-z0-9_-]{1,64}$/;
const SECRET_NAME = /^[A-Za-z_][A-Za-z0-9_]*$/;
const HEADER_NAME = /^[A-Za-z0-9!#$%&'*+.^_`|~-]+$/;
const RESERVED = ["browser", "case-law"];

/** One save as the fake had it, its values hidden. */
export type FakeSave = { previousName: string | null; server: McpServerDraft };

export class FakeTools implements ToolsCommands {
  view: McpToolsView;
  /** Each save, its set values as "(set)". */
  readonly saves: FakeSave[] = [];
  readonly removed: string[] = [];
  readonly toggled: { name: string; enabled: boolean }[] = [];
  /** The next command's refusal, whatever it carries; cleared once used. */
  refuseNext: McpToolsError | null = null;
  readonly previewRemote: boolean;
  /** The values saved per server and secret name. Never shown. */
  private readonly values = new Map<string, string>();

  constructor(view: McpToolsView, { previewRemote = false }: { previewRemote?: boolean } = {}) {
    this.view = { control: view.control, servers: [...view.servers] };
    this.previewRemote = previewRemote;
  }

  read(): Promise<McpToolsView> {
    return Promise.resolve(structuredClone(this.view));
  }

  private refused(): McpToolsError | null {
    const refusal = this.refuseNext;
    this.refuseNext = null;
    if (refusal !== null) return refusal;
    return this.view.control === "lawyer" ? null : "lockedByFirm";
  }

  save(previousName: string | null, server: McpServerDraft): Promise<ToolsAnswer> {
    this.saves.push({
      previousName,
      server: {
        ...server,
        env: server.env.map((e) => ("set" in e.value ? { ...e, value: { set: "(set)" } } : e)),
        headers: server.headers.map((h) =>
          "set" in h.value ? { ...h, value: { set: "(set)" } } : h,
        ),
      },
    });
    return Promise.resolve(this.check(previousName, server));
  }

  private check(previousName: string | null, server: McpServerDraft): ToolsAnswer {
    const refusal = this.refused();
    if (refusal !== null) return refusal;
    const name = server.name.trim();
    if (name === "") return "nameEmpty";
    if (!NAME.test(name) || name.includes("__") || name.endsWith("_")) return "nameInvalid";
    if (RESERVED.includes(name)) return "nameReserved";
    if (name !== previousName && this.view.servers.some((s) => s.name === name)) return "nameTaken";
    if (server.kind === "local" && !isFullPath(server.command ?? "")) return "commandNotAbsolute";
    if (server.kind === "remote" && !/^https?:\/\/[^\s/]+/.test(server.url ?? ""))
      return "urlInvalid";
    if (server.env.some((e) => !SECRET_NAME.test(e.name))) return "envNameInvalid";
    if (server.headers.some((h) => !HEADER_NAME.test(h.name))) return "headerNameInvalid";
    const before = this.view.servers.find((s) => s.name === previousName);
    // the values this save leaves: each set one, and each kept one from before; the rest are deleted
    const kept = new Map<string, string>();
    const secrets = (edits: McpServerDraft["env"], kind: string) =>
      edits.map((e) => {
        const was = this.values.get(`${previousName ?? ""} ${kind} ${e.name}`);
        const value = "set" in e.value ? e.value.set : was;
        if (value !== undefined) kept.set(`${name} ${kind} ${e.name}`, value);
        return { name: e.name, saved: value !== undefined };
      });
    const view: McpServerView = {
      name,
      kind: server.kind,
      command: server.kind === "local" ? server.command : null,
      args: server.kind === "local" ? server.args : [],
      url: server.kind === "remote" ? server.url : null,
      env: secrets(server.env, "env"),
      headers: server.kind === "remote" ? secrets(server.headers, "header") : [],
      enabled: before?.enabled ?? server.enabled,
    };
    for (const key of this.values.keys())
      if (key.startsWith(`${previousName ?? name} `) || key.startsWith(`${name} `))
        this.values.delete(key);
    for (const [key, value] of kept) this.values.set(key, value);
    this.view.servers = [
      ...this.view.servers.filter((s) => s.name !== previousName && s.name !== name),
      view,
    ].sort((a, b) => (a.name < b.name ? -1 : a.name > b.name ? 1 : 0));
    return null;
  }

  remove(name: string): Promise<ToolsAnswer> {
    const refusal = this.refused();
    if (refusal !== null) return Promise.resolve(refusal);
    this.removed.push(name);
    this.view.servers = this.view.servers.filter((s) => s.name !== name);
    return Promise.resolve(null);
  }

  setEnabled(name: string, enabled: boolean): Promise<ToolsAnswer> {
    const refusal = this.refused();
    if (refusal !== null) return Promise.resolve(refusal);
    this.toggled.push({ name, enabled });
    this.view.servers = this.view.servers.map((s) => (s.name === name ? { ...s, enabled } : s));
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
  url: null,
  env: [],
  headers: [],
  enabled: true,
  ...over,
});
