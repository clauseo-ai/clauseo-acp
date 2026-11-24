// What the place asks of the host (library L2): set_addon_on, set_addon_scope and remove_addon,
// one change at a time. While a change is with the host its row shows as it will be; once the host
// answers done, the row stays so until the list is read again (the host's addons_changed), and
// the list then speaks. A refusal puts the row back at once and says one plain line for its code.
import type { Lang } from "@clauseo/components/lib/display.ts";
import { lb } from "@clauseo/components/lib/words-library.ts";
import { useEffect, useRef, useState } from "react";
import type { HostTransport } from "../transport/host-transport.ts";
import type { AddonView } from "./addons.ts";
import { afterChange, type AddonChange } from "./scope-model.ts";

const keyOf = (a: Pick<AddonView, "kind" | "id">) => `${a.kind}:${a.id}`;

const codeOf = (error: unknown): string =>
  typeof error === "object" && error !== null && "code" in error
    ? String((error as { code: unknown }).code)
    : "internal";

/** The one line a refusal says, by its code. */
export function refusalLine(lang: Lang, code: string, name: string): string {
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

/** A refused change: its row as it was, and the host's code. */
export type Refusal = { key: string; code: string; a: AddonView };

export type AddonActs = {
  /** The list with a held change shown. */
  items: readonly AddonView[] | null;
  /** Whether a change is with the host. */
  busy: boolean;
  /** The last refusal, until the next change. */
  refusal: Refusal | null;
  /** Sends `changes` for `a` in order, stopping at the first refusal. */
  send: (a: AddonView, changes: readonly AddonChange[]) => void;
};

/** The place's changes over `items`, the list as read. */
export function useAddonActs(
  transport: HostTransport,
  items: readonly AddonView[] | null,
): AddonActs {
  const [held, setHeld] = useState<Held | null>(null);
  const [refusal, setRefusal] = useState<Refusal | null>(null);
  // the list as last rendered, so done holds the row against the list of its moment
  const latest = useRef(items);
  useEffect(() => {
    latest.current = items;
  }, [items]);
  // once done, the list read after it speaks: the held row goes when the list changes
  const live = held !== null && (held.base === null || held.base === items) ? held : null;
  const shown =
    live === null || items === null
      ? items
      : items.flatMap((a) => (keyOf(a) !== live.key ? [a] : live.view === null ? [] : [live.view]));
  const send = (a: AddonView, changes: readonly AddonChange[]) => {
    if (held?.base === null || changes.length === 0) return;
    const key = keyOf(a);
    const remove = changes.some((c) => "remove_addon" in c);
    const view = remove ? null : changes.reduce(afterChange, a);
    setRefusal(null);
    setHeld({ key, view, base: null });
    void (async () => {
      try {
        for (const command of changes)
          await transport.dispatch({ command_id: crypto.randomUUID(), command });
      } catch (error: unknown) {
        setHeld(null);
        setRefusal({ key, code: codeOf(error), a });
        return;
      }
      setHeld({ key, view, base: latest.current });
    })();
  };
  return { items: shown, busy: held?.base === null, refusal, send };
}
