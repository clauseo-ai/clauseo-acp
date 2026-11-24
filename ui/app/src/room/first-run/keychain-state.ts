// The keychain row's pointer's state (keychain-pointer.tsx): what points at the keychain row, and the hook over the
// window's one permissions read (permissions-host.ts's heardOf).
import { useEffect, useState, useSyncExternalStore } from "react";
import { useDisplay } from "@clauseo/components/lib/display.ts";
import { isKeyNotRead } from "@clauseo/components/lib/key-not-read.ts";
import { fl } from "@clauseo/components/lib/words-first-launch.ts";
import type { HostTransport } from "../../transport/host-transport.ts";
import { heardOf, keychainRow, permissionsHostOf } from "./permissions-host.ts";

/** Host codes for a keychain save or removal refused with dialogs off (sign-in's save, sign-out's clear, a tool's
 * secret save). agent.key_denied answers a refused agent key save or removal too.
 * TODO(stage E / host thread): the host thread lists the other codes here. */
export const KEYCHAIN_CODES: string[] = ["agent.key_denied"];

/** A failure the keychain row may be behind: an unreadable key, or a code in KEYCHAIN_CODES. */
export const pointsToKeychain = (code: string | undefined): code is string =>
  isKeyNotRead(code) || (code !== undefined && KEYCHAIN_CODES.includes(code));

export type Keychain = { line: string; asking: boolean; allow: () => void };

/** The keychain row's pointer, or null while it needs nothing. */
export function useKeychain(transport: HostTransport): Keychain | null {
  const { lang } = useDisplay();
  const h = heardOf(transport);
  const row = keychainRow(useSyncExternalStore(h.subscribe, () => h.rows));
  const [state, setState] = useState<"idle" | "asking" | "failed">("idle");
  // a window with no read yet (Settings, where Privacy may not be open) makes the one read, shared with the step's
  useEffect(() => {
    if (h.rows === null)
      permissionsHostOf(transport)
        .list()
        .catch(() => {});
  }, [h, transport]);
  if (row === null) return null;
  const asking = state === "asking" || row.state === "asking";
  const allow = () => {
    if (asking) return;
    setState("asking");
    permissionsHostOf(transport)
      .ask("keychain")
      .then(
        () => setState("idle"),
        () => setState("failed"),
      );
  };
  const line = asking
    ? fl(lang, "macOS is asking. Type your Mac's login password, then choose Always Allow.")
    : state === "failed"
      ? fl(lang, "Clauseo couldn't ask macOS. Try again.")
      : h.once
        ? fl(lang, "macOS allowed it once only. Press Allow again and choose Always Allow.")
        : fl(lang, "Clauseo needs your Mac's permission to read its saved keys.");
  return { line, asking, allow };
}
