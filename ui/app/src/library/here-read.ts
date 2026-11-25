// What the matter open last gets (library L2): list_addons {matter}, read each time the list is
// read, its `on` set on each row as `here_on`. Read without a matter, `on` can't say whether an
// item with scope "matter" is turned off itself; the host resolves it for a matter, where an item
// on nowhere is off, switch or not. The two reads show together: a new list waits for its
// matter's read, so a row never shows a switch the matter doesn't get.
import { useEffect, useState } from "react";
import type { HostTransport } from "../transport/host-transport.ts";
import type { MatterId } from "../transport/protocol.ts";
import type { AddonView } from "./addons.ts";

const keyOf = (a: Pick<AddonView, "kind" | "id">) => `${a.kind}:${a.id}`;

/** `read` with each row's `on` in `matter` as `here_on`; rows the matter's read lacks keep none. */
export function withHere(
  read: readonly AddonView[],
  matter: MatterId,
  answer: readonly AddonView[],
): AddonView[] {
  const on = new Map(answer.map((a) => [keyOf(a), a.on]));
  return read.map((a) => {
    const here = on.get(keyOf(a));
    return here === undefined ? a : { ...a, here_on: { matter, on: here } };
  });
}

type Shown = { read: readonly AddonView[]; matter: MatterId; items: readonly AddonView[] };

/** The list as read, with the matter's resolved `on` once its read has answered. Until the first
 * answer the list shows as read; after it, a newer list shows once its own matter's read has
 * answered (or failed, when it shows as read). */
export function useHereOn(
  transport: HostTransport,
  matter: MatterId | null,
  read: readonly AddonView[] | null,
): readonly AddonView[] | null {
  const [shown, setShown] = useState<Shown | null>(null);
  useEffect(() => {
    if (matter === null || read === null) return;
    let live = true;
    const show = (items: readonly AddonView[]) => live && setShown({ read, matter, items });
    transport.query({ list_addons: { matter } }).then(
      (answer) => show(withHere(read, matter, answer.addons?.items ?? [])),
      () => show(read),
    );
    return () => {
      live = false;
    };
  }, [transport, matter, read]);
  if (matter === null || read === null) return read;
  if (shown === null || shown.matter !== matter) return read;
  return shown.items;
}
