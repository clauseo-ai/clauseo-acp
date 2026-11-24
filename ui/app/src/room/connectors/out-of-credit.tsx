// The out-of-credit note's shell (U15): in every open conversation's composer, while Clauseo Case Law is out of
// credit by the one rule (store/case-law-credit.ts: the host says so, or the balance read is 0), unless this
// conversation hid it (store/connectors.ts keeps which, in memory). U12's limit warning takes the notice line first
// (limits/limit-warning.tsx's fallback): it affects every answer. While the note shows, the main window gaining focus
// asks for one balance read, so a top-up made elsewhere clears it; an event, not a timer. Once credits arrive after
// this conversation's turn was refused for them, "Credits added. Ask again to search case law." shows instead until
// its next message; nothing resends. Add credits opens the host's pay link through the app's link path (the native
// confirm: the link came with a refused search's answer, so the host's own OpenPayLink, which skips nothing but trusts
// only AddCredits' link, can't take it), else Settings. The notes are in the thread's chunk
// (thread/out-of-credit-note.tsx, re-exported by list.tsx), with their words, as U12's warning is.
import { lazy, Suspense, useEffect } from "react";
import { late } from "@clauseo/components/lib/late.ts";
import {
  CASE_LAW,
  connectorsStoreOf,
  useCreditsAdded,
  useOutOfCredit,
} from "../../store/connectors.ts";
import type { HostTransport } from "../../transport/host-transport.ts";
import { refreshBalance } from "./main-commands.ts";

/** From the thread's chunk, which the open thread has loaded already. */
const OutOfCreditNote = lazy(() =>
  late(import("../thread/list.tsx")).then((m) => ({ default: m.OutOfCreditNote })),
);
const CreditsAddedNote = lazy(() =>
  late(import("../thread/list.tsx")).then((m) => ({ default: m.CreditsAddedNote })),
);

/** The note for `conversation` (its thread, or its draft's key before it has one); nothing when it doesn't show. */
export function OutOfCredit({
  transport,
  conversation,
  onFocusField,
}: {
  transport: HostTransport;
  conversation: string | null;
  /** Puts the focus in the composer's field: Hide takes its own button away. */
  onFocusField: () => void;
}) {
  const store = connectorsStoreOf(transport);
  const out = useOutOfCredit(store, conversation);
  const added = useCreditsAdded(store, conversation);
  const shows = out !== null;
  useEffect(() => {
    if (!shows) return;
    const onFocus = () => void refreshBalance(transport, CASE_LAW);
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, [transport, shows]);
  if (out === null)
    return added ? (
      <Suspense fallback={null}>
        <CreditsAddedNote />
      </Suspense>
    ) : null;
  const open = transport.openSettings;
  const payUrl = out.payUrl;
  return (
    <Suspense fallback={null}>
      <OutOfCreditNote
        credits={out.credits}
        onAdd={
          // the host's pay link through the app's link path, always behind its native confirm (the link came with a
          // refused search's answer); else Settings' Add credits
          payUrl !== null
            ? () => void transport.openExternal(payUrl, undefined, true).catch(() => {})
            : open === undefined
              ? null
              : () => void open.call(transport, "connectors").catch(() => {})
        }
        onHide={
          conversation === null
            ? null
            : () => {
                onFocusField();
                store.hide(conversation);
              }
        }
      />
    </Suspense>
  );
}
