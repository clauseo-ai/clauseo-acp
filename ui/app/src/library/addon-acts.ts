// What the place asks of the host (library L2): set_addon_on, set_addon_scope and remove_addon,
// one change at a time per row. While a change is with the host its row shows as it will be and
// its controls wait; once the host answers done, the row stays so until the list is read again
// (the host's addons_changed), and the list then speaks. Each row holds its own change, so a
// second row's change leaves the first as it is. A refusal puts the row back at once and says one
// plain line for its code.
import type { Lang } from "@clauseo/components/lib/display.ts";
import { lb } from "@clauseo/components/lib/words-library.ts";
import { useEffect, useRef, useState } from "react";
import type { HostTransport } from "../transport/host-transport.ts";
import type { MatterId } from "../transport/protocol.ts";
import type { AddonView } from "./addons.ts";
import { quietly } from "../room/open.ts";
import { afterChange, type AddonChange, type Lens } from "./scope-model.ts";
import type { SwitchActs } from "./switches.tsx";

const keyOf = (a: Pick<AddonView, "kind" | "id">) => `${a.kind}:${a.id}`;

const codeOf = (error: unknown): string =>
  typeof error === "object" && error !== null && "code" in error
    ? String((error as { code: unknown }).code)
    : "internal";

/** The one line a refusal says, by its code; for every matter's off gone half through, where the
 * item is still on. */
export function refusalLine(lang: Lang, code: string, name: string, partial?: string): string {
  if (partial !== undefined)
    return lb(lang, "{name} is still on in {matter}. Clauseo couldn't turn it off there.", {
      name,
      matter: partial,
    });
  switch (code) {
    case "addon.unknown":
      return lb(lang, "Clauseo couldn't find {name} on this computer, so nothing changed.", {
        name,
      });
    case "addon.built_in":
      return lb(lang, "{name} comes with Clauseo, so this can't be changed.", { name });
    case "settings.locked":
      return lb(lang, "Your firm decides this for {name}, so nothing changed.", { name });
    default:
      return lb(lang, "Clauseo couldn't make that change to {name}. Try again.", { name });
  }
}

/** A change with the host, or done and waiting for the list to follow. */
type Held = {
  key: string;
  /** The row as it will be; null for a remove, which shows nothing until the list follows. */
  view: AddonView | null;
  /** The list as it was when the host answered done; null while the host hasn't. */
  base: readonly AddonView[] | null;
};

/** A refused change: its row as it was, and the host's code; `partial`, the matter it is still on
 * in when every matter's off went half through (the scope set, the matter's off refused). */
export type Refusal = { key: string; code: string; a: AddonView; partial: MatterId | null };

export type AddonActs = {
  /** The list with the held changes shown. */
  items: readonly AddonView[] | null;
  /** Whether a change to `a` is with the host. */
  busy: (a: Pick<AddonView, "kind" | "id">) => boolean;
  /** The last refusal, until the next change. */
  refusal: Refusal | null;
  /** Sends `changes` for `a` in order, stopping at the first refusal; false, sending nothing,
   * while a change to `a` is with the host. */
  send: (a: AddonView, changes: readonly AddonChange[]) => boolean;
};

/** The place's changes over `items`, the list as read. */
export function useAddonActs(
  transport: HostTransport,
  items: readonly AddonView[] | null,
): AddonActs {
  const [held, setHeld] = useState<ReadonlyMap<string, Held>>(new Map());
  const [refusal, setRefusal] = useState<Refusal | null>(null);
  // the list as last rendered, so done holds the row against the list of its moment
  const latest = useRef(items);
  useEffect(() => {
    latest.current = items;
  }, [items]);
  // the rows with the host now, read at once: a second press before the next render waits too
  const sending = useRef(new Set<string>());
  const hold = (key: string, next: Held | null) =>
    setHeld((was) => {
      const map = new Map(was);
      if (next === null) map.delete(key);
      else map.set(key, next);
      return map;
    });
  // once done, the list read after it speaks: a held row goes when the list changes
  const live = (key: string) => {
    const h = held.get(key);
    return h !== undefined && (h.base === null || h.base === items) ? h : null;
  };
  const shown =
    items === null || held.size === 0
      ? items
      : items.flatMap((a) => {
          const h = live(keyOf(a));
          return h === null ? [a] : h.view === null ? [] : [h.view];
        });
  const send = (a: AddonView, changes: readonly AddonChange[]) => {
    const key = keyOf(a);
    if (sending.current.has(key) || changes.length === 0) return false;
    sending.current.add(key);
    const remove = changes.some((c) => "remove_addon" in c);
    const view = remove ? null : changes.reduce(afterChange, a);
    setRefusal(null);
    hold(key, { key, view, base: null });
    void (async () => {
      let done = 0;
      try {
        for (const command of changes) {
          await transport.dispatch({ command_id: crypto.randomUUID(), command });
          done += 1;
        }
      } catch (error: unknown) {
        // what went through stands, as the host will list it; the rest is put back
        const through = changes.slice(0, done);
        sending.current.delete(key);
        hold(
          key,
          done === 0 ? null : { key, view: through.reduce(afterChange, a), base: latest.current },
        );
        const scoped = through.map((c) => ("set_addon_scope" in c ? c.set_addon_scope : null));
        const scope = scoped.find((c) => c !== null)?.scope;
        const partial = scope === undefined || scope === "all" ? null : scope.matter;
        setRefusal({ key, code: codeOf(error), a, partial });
        return;
      }
      sending.current.delete(key);
      hold(key, { key, view, base: latest.current });
    })();
    return true;
  };
  return { items: shown, busy: (a) => held.get(keyOf(a))?.base === null, refusal, send };
}

/** The controls' acts over the transport: the lens, the matters, the sender, and Settings. */
export function switchActs(
  transport: HostTransport,
  lens: Lens,
  names: ReadonlyMap<MatterId, string>,
  known: MatterId | null,
  changes: Pick<AddonActs, "send" | "busy">,
): SwitchActs {
  const open = transport.openSettings;
  return {
    lens,
    names,
    first: known ?? names.keys().next().value ?? null,
    send: changes.send,
    busy: changes.busy,
    openSettings:
      open === undefined
        ? null
        : (section) => void quietly("open_settings", open.call(transport, section)),
  };
}
