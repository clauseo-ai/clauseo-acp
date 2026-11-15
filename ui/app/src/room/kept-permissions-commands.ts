// A matter's kept permissions: the rules "Always for this matter" kept for Claude's permission
// requests, read with `kept_permissions` and taken back one at a time with
// `remove_kept_permission` (crates/desktop/src/bridge/admit.rs admits both from the main window
// alone). The host reads each rule into a kind and a subject, so nothing here parses rule text:
// a removal sends back the exact `rule` the list gave.
import type { Lang } from "@clauseo/components/lib/display.ts";
import { isolate } from "@clauseo/components/lib/words-core.ts";
import { kp } from "@clauseo/components/lib/words-kept.ts";
import { useEffect, useState, useSyncExternalStore } from "react";
import type { HostTransport } from "../transport/host-transport.ts";
import type {
  HostCommand,
  HostQuery,
  KeptPermission,
  MatterId,
  QueryResult,
} from "../transport/protocol.ts";

type KeptQuery = Extract<HostQuery, { kept_permissions: unknown }>;
type RemoveCommand = Extract<HostCommand, { remove_kept_permission: unknown }>;

/** A refusal's code; `internal` for anything that carries none. */
const codeOf = (error: unknown): string =>
  typeof error === "object" && error !== null && "code" in error
    ? String((error as { code: unknown }).code)
    : "internal";

/** One kept rule in plain words, by its kind: "Run: npm test", "Any page on example.com". */
export function keptLine(lang: Lang, kept: KeptPermission): string {
  const subject = isolate(kept.subject);
  switch (kept.kind) {
    case "run":
      return kp(lang, "Run: {command}", { command: subject });
    case "edit":
      return kp(lang, "Edit: {file}", { file: subject });
    case "read":
      return kp(lang, "Read: {file}", { file: subject });
    case "site":
      return kp(lang, "Any page on {site}", { site: subject });
    case "tool":
      return kp(lang, "Use tool: {server} {tool}", {
        server: isolate(kept.server ?? ""),
        tool: subject,
      });
  }
}

export type KeptState = {
  /** The list as last read; null before the first answer. */
  kept: readonly KeptPermission[] | null;
  /** The code of the last read or removal the host refused; null after one that went through. */
  failed: string | null;
  /** The rule whose Remove is with the host. */
  removing: string | null;
};

const FIRST: KeptState = { kept: null, failed: null, removing: null };

/** The list's state for `matter`: `read` asks again, `remove` takes one rule back and reads
 * again. An answer stands only while no later read was asked for. */
function keptOver(transport: HostTransport, matter: MatterId) {
  let state = FIRST;
  let asked = 0;
  const listeners = new Set<() => void>();
  const set = (patch: Partial<KeptState>) => {
    state = { ...state, ...patch };
    for (const listener of listeners) listener();
  };
  const read = async (): Promise<void> => {
    const n = (asked += 1);
    const query: KeptQuery = { kept_permissions: { matter } };
    try {
      const answer: QueryResult = await transport.query(query);
      const kept = typeof answer === "object" ? answer.kept_permissions : undefined;
      if (n === asked) set({ kept: kept ?? [], failed: null });
    } catch (error: unknown) {
      if (n === asked) set({ failed: codeOf(error) });
    }
  };
  const remove = async (rule: string): Promise<void> => {
    if (state.removing !== null) return;
    set({ removing: rule });
    let failed: string | null = null;
    const command: RemoveCommand = { remove_kept_permission: { matter, rule } };
    try {
      await transport.dispatch({ command_id: crypto.randomUUID(), command });
    } catch (error: unknown) {
      failed = codeOf(error);
    }
    await read();
    set({ removing: null, ...(failed === null ? {} : { failed }) });
  };
  return {
    subscribe: (listener: () => void) => {
      listeners.add(listener);
      return () => void listeners.delete(listener);
    },
    state: () => state,
    read,
    remove,
  };
}

/** `matter`'s kept permissions, read as the list opens and after each Remove. */
export function useKeptPermissions(
  transport: HostTransport,
  matter: MatterId,
): KeptState & { remove: (rule: string) => void } {
  const [list] = useState(() => keptOver(transport, matter));
  const state = useSyncExternalStore(list.subscribe, list.state, list.state);
  useEffect(() => void list.read(), [list]);
  return { ...state, remove: (rule) => void list.remove(rule) };
}
