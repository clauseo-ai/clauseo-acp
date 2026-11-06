// The case-law hint (U15) under an answer: when the host sends `case_law_hint { matter, thread, turn }`, after a turn
// searched the web for case law in a matter without the connector, a small card under that turn's answer, in that
// thread only. The host decides when; the app only draws it (store/connectors.ts keeps the hints heard, in memory).
// "Set up in Settings" opens Settings at Connectors and leaves the hint; "Don't show again" sends
// `dismiss_case_law_hint { matter }`, the one command it has (room/connectors/main-commands.ts), and the hint goes at
// once. NoticeCard's look with no rows, not waiting, the search glyph in its tile. In the thread's chunk (list.tsx
// draws it), with its words and the money format, so the room's first load carries none of them.
import { use } from "react";
import { NoticeCard } from "@clauseo/components/clauseo/limit-note.tsx";
import { IconSearch } from "@clauseo/components/icons/icons.tsx";
import { useDisplay } from "@clauseo/components/lib/display.ts";
import { rupees } from "@clauseo/components/lib/money.ts";
import { cr } from "@clauseo/components/lib/words-connectors-room.ts";
import { Button } from "@clauseo/components/ui/button.tsx";
import type { CaseLawHint as Hint } from "../host-next.ts";
import {
  CASE_LAW,
  connectorsStoreOf,
  useCaseLawHint,
  useConnector,
  type ConnectorsStore,
} from "../../store/connectors.ts";
import type { HostTransport } from "../../transport/host-transport.ts";
import type { StreamId, TurnId } from "../../transport/protocol.ts";
import { dismissHint } from "../connectors/main-commands.ts";
import { SourcesTransport } from "./sources.ts";

const THREAD = "thread:";

/** The hint for a turn of the stream's thread, when the host sent one and the state lets it show; nothing otherwise. */
export function CaseLawHintSlot({ stream, turn }: { stream: StreamId; turn: TurnId | null }) {
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
  const store = connectorsStoreOf(transport);
  const hint = useCaseLawHint(store, thread, turn);
  return hint === undefined ? null : <HintCard transport={transport} store={store} hint={hint} />;
}

function HintCard({
  transport,
  store,
  hint,
}: {
  transport: HostTransport;
  store: ConnectorsStore;
  hint: Hint;
}) {
  const price = useConnector(store, CASE_LAW)?.price_paise;
  const open = transport.openSettings;
  return (
    <CaseLawHint
      price={price ?? null}
      onSetUp={
        open === undefined ? null : () => void open.call(transport, "connectors").catch(() => {})
      }
      onDismiss={() => {
        void dismissHint(transport, hint.matter);
        store.dropHints(hint.matter);
      }}
    />
  );
}

/** The card: the offer, "from" the smallest pack's gross price when known, and its two buttons. */
export function CaseLawHint({
  price,
  onSetUp,
  onDismiss,
}: {
  /** `price_paise`, GST included; null when the host gave none. */
  price: number | null;
  onSetUp: (() => void) | null;
  onDismiss: () => void;
}) {
  const { lang } = useDisplay();
  return (
    <div className="contents" data-testid="case-law-hint">
      <NoticeCard
        icon={<IconSearch size={18} />}
        label={cr(lang, "Indian Kanoon suggestion")}
        title={cr(lang, "For judgments, Clauseo can search Indian Kanoon directly.")}
        sub={
          price === null
            ? cr(lang, "It's a paid add-on.")
            : cr(lang, "It's a paid add-on, from {price}, including GST.", { price: rupees(price) })
        }
        actions={
          <>
            {onSetUp !== null && (
              <Button kind="tone" size="small" data-testid="case-law-set-up" onClick={onSetUp}>
                {cr(lang, "Set up in Settings")}
              </Button>
            )}
            <Button kind="quiet" size="small" data-testid="case-law-dismiss" onClick={onDismiss}>
              {cr(lang, "Don't show again")}
            </Button>
          </>
        }
      />
    </div>
  );
}
