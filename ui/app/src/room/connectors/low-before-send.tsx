// The running-low line by the composer (U15, 24 Nov ruling): while the case-law connector's credit is low by the host's
// flag (crates/host/src/connectors/state.rs, under ten searches' worth), its words, "Running low. Add credits to keep
// searching.", show under the field before a question is sent, not only under an answer whose turn ran as it turned
// low. Not while that line shows under a turn of this thread, and never with the out-of-credit note, which comes first
// (store/connectors.ts decides). U12's limit warning takes the notice line before it, as it does the out-of-credit
// note. The line itself is in the thread's chunk (thread/low-credit.tsx, re-exported by list.tsx), with its words.
import { lazy, Suspense } from "react";
import { late } from "@clauseo/components/lib/late.ts";
import { connectorsStoreOf, useLowBeforeSend } from "../../store/connectors.ts";
import type { HostTransport } from "../../transport/host-transport.ts";

/** From the thread's chunk, which the open thread has loaded already. */
const LowCredit = lazy(() =>
  late(import("../thread/list.tsx")).then((m) => ({ default: m.LowCredit })),
);

/** The line for `thread` (null for a draft); nothing while it doesn't show. */
export function LowBeforeSend({
  transport,
  thread,
}: {
  transport: HostTransport;
  thread: string | null;
}) {
  const shows = useLowBeforeSend(connectorsStoreOf(transport), thread);
  if (!shows) return null;
  const open = transport.openSettings;
  return (
    <Suspense fallback={null}>
      <LowCredit
        testId="low-credit-before"
        onAdd={
          open === undefined ? null : () => void open.call(transport, "connectors").catch(() => {})
        }
      />
    </Suspense>
  );
}
