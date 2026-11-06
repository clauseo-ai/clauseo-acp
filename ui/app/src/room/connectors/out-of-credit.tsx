// The out-of-credit note's shell (U15): in every open conversation's composer, while connectors are on and a listed
// connector is out of credit, unless this conversation hid it (store/connectors.ts keeps which, in memory). U12's limit
// warning takes the notice line first (limits/limit-warning.tsx's fallback): it affects every answer. While the note
// shows, the main window gaining focus asks for one balance read, so a top-up made elsewhere clears it; an event, not a
// timer. The note itself is in the thread's chunk (thread/out-of-credit-note.tsx, re-exported by list.tsx), with its
// words, as U12's warning is.
import { lazy, Suspense, useEffect } from "react";
import { late } from "@clauseo/components/lib/late.ts";
import { connectorsStoreOf, useOutOfCredit } from "../../store/connectors.ts";
import type { HostTransport } from "../../transport/host-transport.ts";
import { refreshBalance } from "./main-commands.ts";

/** From the thread's chunk, which the open thread has loaded already. */
const OutOfCreditNote = lazy(() =>
  late(import("../thread/list.tsx")).then((m) => ({ default: m.OutOfCreditNote })),
);

/** The note for `conversation` (its thread, or its draft's key before it has one); nothing when it doesn't show. */
export function OutOfCredit({
  transport,
  conversation,
}: {
  transport: HostTransport;
  conversation: string | null;
}) {
  const store = connectorsStoreOf(transport);
  const id = useOutOfCredit(store, conversation);
  useEffect(() => {
    if (id === null) return;
    const onFocus = () => void refreshBalance(transport, id);
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, [transport, id]);
  if (id === null) return null;
  const open = transport.openSettings;
  return (
    <Suspense fallback={null}>
      <OutOfCreditNote
        onAdd={
          open === undefined ? null : () => void open.call(transport, "connectors").catch(() => {})
        }
        onHide={conversation === null ? null : () => store.hide(conversation)}
      />
    </Suspense>
  );
}
