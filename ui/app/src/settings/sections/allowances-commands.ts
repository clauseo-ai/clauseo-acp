// Settings' remembered answers (R18): the `list_allowances` read, kept up to date from each matter's
// stream, and `forget_allowance`, the one R18 command this window sends (crates/desktop/src/bridge/
// admit.rs admits it here, and refuses switching the Safe workspace off or on). The type below is
// the window's whole vocabulary for R18, so there is no typed way to send anything else from here.
// The list is read as the section mounts, after each Forget answers, and whenever the window gains
// focus or becomes visible again: an answer is recorded through a native dialog over the main
// window, so the lawyer comes back to this window after it. No matter's stream is followed: with
// many matters that would hold many subscriptions in a window that is mostly idle.
import { useEffect, useState, useSyncExternalStore } from "react";
import type { HostTransport } from "../../transport/host-transport.ts";
import type {
  AllowanceId,
  HostCommand,
  MatterAllowance,
  MatterId,
} from "../../transport/protocol.ts";

/** What Settings sends for the remembered answers. */
export type SettingsAllowanceCommand = Extract<HostCommand, { forget_allowance: unknown }>;

/** Each matter's remembered answers, in the order the host lists them. */
export type ByMatter = ReadonlyMap<MatterId, readonly MatterAllowance[]>;

export type AllowancesState = {
  /** The list as last read; null before the first answer. */
  byMatter: ByMatter | null;
  /** The code of the last read or Forget the host refused; null after one that went through. */
  failed: string | null;
  /** The answer whose Forget is with the host. */
  forgetting: AllowanceId | null;
};

/** A refusal's code; `internal` for anything that carries none. */
const codeOf = (error: unknown): string =>
  typeof error === "object" && error !== null && "code" in error
    ? String((error as { code: unknown }).code)
    : "internal";

const FIRST: AllowancesState = { byMatter: null, failed: null, forgetting: null };

/** The rows a list groups by matter, keeping the host's order. */
export function groupByMatter(list: readonly MatterAllowance[]): ByMatter {
  const out = new Map<MatterId, MatterAllowance[]>();
  for (const row of list) out.set(row.matter, [...(out.get(row.matter) ?? []), row]);
  return out;
}

/** The list's state over a transport. */
function allowancesOver(transport: HostTransport) {
  let state = FIRST;
  const listeners = new Set<() => void>();
  const set = (patch: Partial<AllowancesState>) => {
    state = { ...state, ...patch };
    for (const listener of listeners) listener();
  };
  // each read's answer stands only while no later read was asked for
  let asked = 0;
  const read = async (): Promise<void> => {
    const n = (asked += 1);
    try {
      const answer = await transport.query("list_allowances");
      if (n === asked) set({ byMatter: groupByMatter(answer.allowances ?? []), failed: null });
    } catch (error: unknown) {
      if (n === asked) set({ failed: codeOf(error) });
    }
  };
  const forget = async (row: MatterAllowance): Promise<void> => {
    if (state.forgetting !== null) return;
    set({ forgetting: row.allowance });
    const command: SettingsAllowanceCommand = {
      forget_allowance: { matter: row.matter, allowance: row.allowance },
    };
    let failed: string | null = null;
    try {
      await transport.dispatch({ command_id: crypto.randomUUID(), command });
    } catch (error: unknown) {
      failed = codeOf(error);
    }
    await read();
    set({ forgetting: null, ...(failed === null ? {} : { failed }) });
  };
  return {
    subscribe: (listener: () => void) => {
      listeners.add(listener);
      return () => void listeners.delete(listener);
    },
    state: () => state,
    read,
    forget,
  };
}

/** The remembered answers: read as the section mounts, after each Forget, and whenever the window
 * gains focus or becomes visible. */
export function useAllowances(
  transport: HostTransport,
): AllowancesState & { forget: (row: MatterAllowance) => void } {
  const [list] = useState(() => allowancesOver(transport));
  const state = useSyncExternalStore(list.subscribe, list.state, list.state);
  useEffect(() => {
    void list.read();
    const again = () => void list.read();
    const shown = () => {
      if (document.visibilityState === "visible") again();
    };
    window.addEventListener("focus", again);
    document.addEventListener("visibilitychange", shown);
    return () => {
      window.removeEventListener("focus", again);
      document.removeEventListener("visibilitychange", shown);
    };
  }, [list]);
  return { ...state, forget: (row) => void list.forget(row) };
}
