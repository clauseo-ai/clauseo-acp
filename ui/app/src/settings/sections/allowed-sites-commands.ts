// Settings' allowed sites (0053, PR 2): the `allowed_sites` read, and `revoke_allowed_site`, the
// one unsafe-site command this window sends (crates/desktop/src/bridge/admit.rs admits it here, and
// refuses the room's `allow_blocked_site`). The type below is the window's whole vocabulary for the
// list, so there is no typed way to send an allow from here. Matter names come from the app feed's
// `matters_changed`, whose first frames are the whole list: this window may not read
// `list_matters`.
import { useEffect, useState, useSyncExternalStore } from "react";
import { nextHostOf } from "../../room/host-next.ts";
import type { HostTransport } from "../../transport/host-transport.ts";
import type { AllowedSite, HostCommand, MatterId } from "../../transport/protocol.ts";

/** What Settings sends for the unsafe-site list. */
export type SettingsBlocklistCommand = Extract<HostCommand, { revoke_allowed_site: unknown }>;

/** A refusal's code; `internal` for anything that carries none. */
const codeOf = (error: unknown): string =>
  typeof error === "object" && error !== null && "code" in error
    ? String((error as { code: unknown }).code)
    : "internal";

/** Takes an allow back: the host and its reach, and the matter of a matter allow. */
export async function revokeSite(transport: HostTransport, site: AllowedSite): Promise<void> {
  const command: SettingsBlocklistCommand = {
    revoke_allowed_site: {
      host: site.host,
      scope: site.scope,
      ...(site.scope === "matter" && site.matter !== undefined ? { matter: site.matter } : {}),
    },
  };
  await transport.dispatch({ command_id: crypto.randomUUID(), command });
}

export type AllowedSitesState = {
  /** The list as last read; null before the first answer. */
  sites: readonly AllowedSite[] | null;
  /** The code of the last read or revoke the host refused; null after one that went through. */
  failed: string | null;
  /** The site whose Remove is with the host. */
  removing: AllowedSite | null;
  /** The site last removed until Clauseo closes only (`blocklist.revoke_not_saved`): blocked
   * again now, but Clauseo couldn't save that. Cleared by the next Remove. */
  unsaved: AllowedSite | null;
};

/** The host's answer for a revoke that took but couldn't be saved (0053): an error whose change
 * still applied (0003), so the site is gone from the list and a quiet line says why it may
 * return. */
export const REVOKE_NOT_SAVED = "blocklist.revoke_not_saved";

const FIRST: AllowedSitesState = { sites: null, failed: null, removing: null, unsaved: null };

/** The list's state over a transport: `read` asks again, `remove` revokes one and reads again. An
 * answer stands only while no later read was asked for. */
function allowedSitesOver(transport: HostTransport) {
  let state = FIRST;
  let asked = 0;
  const listeners = new Set<() => void>();
  const set = (patch: Partial<AllowedSitesState>) => {
    state = { ...state, ...patch };
    for (const listener of listeners) listener();
  };
  const read = async (): Promise<void> => {
    const n = (asked += 1);
    try {
      const answer = await transport.query("allowed_sites");
      if (n === asked) set({ sites: answer.allowed_sites ?? [], failed: null });
    } catch (error: unknown) {
      if (n === asked) set({ failed: codeOf(error) });
    }
  };
  const remove = async (site: AllowedSite): Promise<void> => {
    if (state.removing !== null) return;
    set({ removing: site, unsaved: null });
    let failed: string | null = null;
    let unsaved: AllowedSite | null = null;
    try {
      await revokeSite(transport, site);
    } catch (error: unknown) {
      const code = codeOf(error);
      if (code === REVOKE_NOT_SAVED) unsaved = site;
      else failed = code;
    }
    await read();
    set({ removing: null, unsaved, ...(failed === null ? {} : { failed }) });
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

/** The allowed sites, read as the section opens, after each Remove and whenever the window regains
 * focus. */
export function useAllowedSites(
  transport: HostTransport,
): AllowedSitesState & { remove: (site: AllowedSite) => void } {
  const [list] = useState(() => allowedSitesOver(transport));
  const state = useSyncExternalStore(list.subscribe, list.state, list.state);
  useEffect(() => {
    void list.read();
    const focused = () => void list.read();
    window.addEventListener("focus", focused);
    return () => window.removeEventListener("focus", focused);
  }, [list]);
  return { ...state, remove: (site) => void list.remove(site) };
}

/** Each matter's name, from the app feed's `matters_changed`; empty until the first frames. */
export function useMatterNames(transport: HostTransport): ReadonlyMap<MatterId, string> {
  const [names, setNames] = useState<ReadonlyMap<MatterId, string>>(() => new Map());
  useEffect(
    () =>
      nextHostOf(transport).app?.subscribeApp((event) => {
        if (!("matters_changed" in event)) return;
        const { matters, removed, all } = event.matters_changed;
        setNames((was) => {
          const next = new Map(all ? [] : was);
          for (const id of removed) next.delete(id);
          for (const m of matters) next.set(m.matter, m.name);
          return next;
        });
      }),
    [transport],
  );
  return names;
}
