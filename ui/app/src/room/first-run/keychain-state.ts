// The keychain row's pointer's state (keychain-pointer.tsx): what points at the keychain row, and the hook over the
// window's one permissions read (permissions-host.ts's heardOf).
import { useEffect, useState, useSyncExternalStore } from "react";
import { useDisplay } from "@clauseo/components/lib/display.ts";
import { isKeyNotRead } from "@clauseo/components/lib/key-not-read.ts";
import { fl } from "@clauseo/components/lib/words-first-launch.ts";
import type { HostTransport } from "../../transport/host-transport.ts";
import { awaitsAllow, isAsking, keychainRow } from "./permission-rows.ts";
import { heardOf, pending, permissionsHostOf } from "./permissions-host.ts";

/** Host codes for a keychain save or removal refused with dialogs off (sign-in's save, sign-out's clear, a tool's
 * secret save). agent.key_denied answers a refused agent key save or removal too.
 * TODO(stage E / host thread): the host thread lists the other codes here. */
export const KEYCHAIN_CODES: string[] = ["agent.key_denied"];

/** A failure the keychain row may be behind: an unreadable key, or a code in KEYCHAIN_CODES. */
export const pointsToKeychain = (code: string | undefined): code is string =>
  isKeyNotRead(code) || (code !== undefined && KEYCHAIN_CODES.includes(code));

/** The pointer: its line, whether macOS is asking (Allow disabled), and its Allow, given the press's event so focus can
 * land on the line that takes the button's place once the row is allowed. */
export type Keychain = {
  line: string;
  asking: boolean;
  allow: (event?: { currentTarget: unknown }) => void;
};

/** The pressed button's ancestors, nearest first, up to the body. */
function ancestorsOf(at: unknown): HTMLElement[] {
  const path: HTMLElement[] = [];
  for (
    let el = at instanceof HTMLElement ? at.parentElement : null;
    el !== null;
    el = el.parentElement
  ) {
    if (el === document.body) break;
    path.push(el);
  }
  return path;
}

/** After a grant the pointer's Allow is gone: focus goes to what took its place, in the nearest place still standing
 * (a line marked data-keychain-after, else its first line, else its first button), so it isn't lost to the body. */
function focusAfter(path: HTMLElement[]) {
  setTimeout(() => {
    const place = path.find((el) => el.isConnected);
    if (place === undefined) return;
    const line =
      place.querySelector<HTMLElement>("[data-keychain-after]") ??
      place.querySelector<HTMLElement>('p, [role="status"]') ??
      place.querySelector<HTMLElement>("button:not([disabled])");
    if (line === null) return;
    if (line.tabIndex < 0 && !line.hasAttribute("tabindex")) line.tabIndex = -1;
    line.focus();
  }, 0);
}

/** The keychain row's pointer, or null while it needs nothing (permission-rows.ts's awaitsAllow, the one rule the band
 * stands by too). Its waiting is the row's (permission-rows.ts's isAsking): every screen in the window shows it. */
export function useKeychain(transport: HostTransport): Keychain | null {
  const { lang } = useDisplay();
  const h = heardOf(transport);
  const row = keychainRow(useSyncExternalStore(h.subscribe, () => h.rows));
  const waiting = useSyncExternalStore(h.subscribe, () => pending(h));
  const [failed, setFailed] = useState(false);
  // a window with no read yet (Settings, where Privacy may not be open) makes the one read, shared with the step's
  useEffect(() => {
    if (h.rows === null)
      permissionsHostOf(transport)
        .list()
        .catch(() => {});
  }, [h, transport]);
  if (row === null) return null;
  const asking = isAsking(row, waiting);
  const allow = (event?: { currentTarget: unknown }) => {
    if (asking) return;
    setFailed(false);
    const path = ancestorsOf(event?.currentTarget);
    permissionsHostOf(transport)
      .ask("keychain")
      .then(
        (after) => {
          if (!awaitsAllow(after)) focusAfter(path);
        },
        () => setFailed(true),
      );
  };
  const line = asking
    ? fl(lang, "macOS is asking. Type your Mac's login password, then choose Always Allow.")
    : failed
      ? fl(lang, "Clauseo couldn't ask macOS. Try again.")
      : h.once
        ? fl(lang, "macOS allowed it once only. Press Allow again and choose Always Allow.")
        : fl(lang, "Clauseo needs your Mac's permission to read its saved keys.");
  return { line, asking, allow };
}
