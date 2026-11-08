// The room's unsafe-site command (0053): "Allow" from a notice's confirm, the one command only the
// main window may send (crates/desktop/src/bridge/admit.rs refuses it from Settings). Nothing in
// the settings window imports this file; its revoke lives in
// settings/sections/allowed-sites-commands.ts.
import type { BlockedSitesStore } from "../../store/blocked-sites.ts";
import type { HostTransport } from "../../transport/host-transport.ts";
import type { AllowScope, BlockedSiteId, HostCommand } from "../../transport/protocol.ts";

/** What the room sends for the unsafe-site list. */
export type MainBlocklistCommand = Extract<HostCommand, { allow_blocked_site: unknown }>;

/** A refusal's code; `internal` for anything that carries none. */
export const codeOf = (error: unknown): string =>
  typeof error === "object" && error !== null && "code" in error
    ? String((error as { code: unknown }).code)
    : "internal";

/** The host's answer for an allow that took but couldn't be saved (0053): it lasts until Clauseo
 * closes. An error whose change still applied (0003), so the notice shows it allowed. */
export const ALLOW_NOT_SAVED = "blocklist.allow_not_saved";

/** Allows the site a notice named, as far as `scope`, unless its allow is already with the host.
 * The notice says how it went: allowed (saved or for this run only), or the refusal's code. */
export async function allowSite(
  transport: HostTransport,
  store: BlockedSitesStore,
  blockId: BlockedSiteId,
  scope: AllowScope,
): Promise<void> {
  if (store.notice(blockId)?.sending !== false) return;
  store.set(blockId, { sending: true, refused: null });
  const command: MainBlocklistCommand = { allow_blocked_site: { block_id: blockId, scope } };
  try {
    await transport.dispatch({ command_id: crypto.randomUUID(), command });
    store.set(blockId, { sending: false, allowed: scope, unsaved: false });
  } catch (error: unknown) {
    const code = codeOf(error);
    if (code === ALLOW_NOT_SAVED)
      store.set(blockId, { sending: false, allowed: scope, unsaved: true });
    else store.set(blockId, { sending: false, refused: code });
  }
}
