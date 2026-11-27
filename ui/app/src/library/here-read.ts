// What the matter open last gets (library L2): list_addons {matter}, read each time the list is
// read, its `on` set on each row as `here_on`. The host resolves it for a matter, where an item
// turned off itself is off, switch or not. The two reads show together: a new list waits for its
// matter's read, so a row never shows a switch the matter doesn't get. A failed read keeps the
// matter's last good answer over the new list; with none yet, the place says it couldn't check.
import { useEffect, useState } from "react";
import type { HostTransport } from "../transport/host-transport.ts";
import type { MatterId } from "../transport/protocol.ts";
import type { AddonView } from "./addons.ts";

const keyOf = (a: Pick<AddonView, "kind" | "id">) => `${a.kind}:${a.id}`;

/** A matter's answer: each row's `on` there, by kind and id. */
type Answer = ReadonlyMap<string, boolean>;

/** `read` with each row's `on` in `matter` as `here_on`; rows the answer lacks keep none. */
export function withHere(
  read: readonly AddonView[],
  matter: MatterId,
  answer: Answer,
): AddonView[] {
  return read.map((a) => {
    const here = answer.get(keyOf(a));
    return here === undefined ? a : { ...a, here_on: { matter, on: here } };
  });
}

type Shown = {
  matter: MatterId;
  items: readonly AddonView[];
  /** The matter's last good answer; null while none has come. */
  answer: Answer | null;
};

/** What the place shows: the list with the matter's `on`, and whether no read of it has answered
 * yet (the last one failed with none before it). */
export type MatterRead = { items: readonly AddonView[] | null; unread: boolean };

/** The list as read, with the matter's resolved `on` once its read has answered. Until the first
 * answer the list shows as read; after it, a newer list shows once its own matter's read has
 * answered, or failed, when it shows with the last good answer. */
export function useHereOn(
  transport: HostTransport,
  matter: MatterId | null,
  read: readonly AddonView[] | null,
): MatterRead {
  const [shown, setShown] = useState<Shown | null>(null);
  useEffect(() => {
    if (matter === null || read === null) return;
    let live = true;
    transport.query({ list_addons: { matter } }).then(
      (result) => {
        if (!live) return;
        const rows = result.addons?.items ?? [];
        const answer: Answer = new Map(rows.map((a) => [keyOf(a), a.on]));
        setShown({ matter, items: withHere(read, matter, answer), answer });
      },
      () => {
        if (!live) return;
        setShown((was) => {
          const last = was?.matter === matter ? was.answer : null;
          return {
            matter,
            items: last === null ? read : withHere(read, matter, last),
            answer: last,
          };
        });
      },
    );
    return () => {
      live = false;
    };
  }, [transport, matter, read]);
  if (matter === null || read === null || shown === null || shown.matter !== matter)
    return { items: read, unread: false };
  return { items: shown.items, unread: shown.answer === null };
}
