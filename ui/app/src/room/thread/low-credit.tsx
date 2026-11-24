// The running-low line (U15) under a turn's answer: when the case-law connector's credit turns low while a turn of this
// thread runs, Settings' words, "Running low. Add credits to keep searching.", show once under that turn, with the
// out-of-credit note's "Add credits", which opens Settings at Connectors as the note's does. Low already when the
// turn started, no line: Settings says so. It goes once credit is no longer low, and gives way to the out-of-credit
// note (store/connectors.ts decides). The thread list tells the store which turn runs (running-turn.ts); no timers, no
// commands of its own. In the thread's chunk, with its words, as the case-law hint is.
import { use } from "react";
import { FieldNotice } from "@clauseo/components/clauseo/limit-note.tsx";
import { useDisplay } from "@clauseo/components/lib/display.ts";
import { cw } from "@clauseo/components/lib/words-connectors.ts";
import { cr } from "@clauseo/components/lib/words-connectors-room.ts";
import { Button } from "@clauseo/components/ui/button.tsx";
import { connectorsStoreOf, useLowCredit } from "../../store/connectors.ts";
import type { HostTransport } from "../../transport/host-transport.ts";
import type { StreamId, TurnId } from "../../transport/protocol.ts";
import { SourcesTransport } from "./sources.ts";

const THREAD = "thread:";

/** The line under a turn of the stream's thread, while the store says it shows; nothing otherwise. */
export function LowCreditSlot({ stream, turn }: { stream: StreamId; turn: TurnId | null }) {
  const transport = use(SourcesTransport);
  if (transport === null || !stream.startsWith(THREAD)) return null;
  return <Slot transport={transport} thread={stream.slice(THREAD.length)} turn={turn} />;
}

function Slot({
  transport,
  thread,
  turn,
}: {
  transport: HostTransport;
  thread: string;
  turn: TurnId | null;
}) {
  const shows = useLowCredit(connectorsStoreOf(transport), thread, turn);
  if (!shows) return null;
  const open = transport.openSettings;
  return (
    <LowCredit
      onAdd={
        open === undefined ? null : () => void open.call(transport, "connectors").catch(() => {})
      }
    />
  );
}

/** The line; `onAdd` opens Settings at Connectors (none where the host has no settings window). `testId` names where
 * it stands: "low-credit" under a turn, "low-credit-before" by the composer; its Add credits takes it with "-add". */
export function LowCredit({
  onAdd,
  testId = "low-credit",
}: {
  onAdd: (() => void) | null;
  testId?: string;
}) {
  const { lang } = useDisplay();
  return (
    <div className="contents" data-testid={testId}>
      <FieldNotice
        lead={cw(lang, "Running low. Add credits to keep searching.")}
        action={
          onAdd === null ? null : (
            // as the out-of-credit note's: words at the end of the line, no padding of its own
            <Button
              kind="link"
              size="small"
              className="min-h-0 px-0 py-0 text-primary align-baseline underline underline-offset-2"
              data-testid={`${testId}-add`}
              onClick={onAdd}
            >
              {cr(lang, "Add credits")}
            </Button>
          )
        }
      />
    </div>
  );
}
