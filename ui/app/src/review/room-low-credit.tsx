// The running-low line under a turn (U15), for a look at it in the room: no string of its own, so not a copy entry
// (its words are U15-19's and U15-66's, reviewed where they first show). The real room over FakeTransport: the turn
// runs, the case-law connector signed in and not low, and a second after the room shows, its credit turns low, as the
// host's balance read reports it. Opened as ?cell=room/low-credit (scenes.tsx), as the review's states are.
import { useEffect, useState } from "react";
import { Room } from "../room/room.tsx";
import { frameBatch } from "../store/batch.ts";
import { StoreContext } from "../store/hooks.ts";
import { createViewStore } from "../store/store.ts";
import { agentText, userMessage } from "../testing/stream-script.ts";
import type { ConnectorStatus, ConnectorsState } from "../transport/protocol.ts";
import { threadHost } from "./hosts.ts";

const state = (status: ConnectorStatus): ConnectorsState => ({
  available: true,
  locked: false,
  connectors: [
    {
      id: "case-law",
      name: "Indian Kanoon",
      price_paise: 2_360,
      packs: [{ pack: "p200", credits: 200, paise: 2_360 }],
      status,
    },
  ],
});

/** The room, its turn running, and a second after it shows, the case-law credit turning low. */
export function LowMidTurn() {
  const [transport] = useState(() =>
    threadHost({
      status: "running",
      entries: [
        userMessage(5, 1, "Find Supreme Court judgments on section 138 of the NI Act."),
        agentText(6, 1, "Searching Indian Kanoon for Supreme Court judgments under section 138."),
      ],
      connectors: state({ signed_in: { low: false, balance: { credits: 40 } } }),
    }),
  );
  const [store] = useState(() => createViewStore(transport, { batch: () => frameBatch }));
  useEffect(() => {
    const timer = setTimeout(
      () =>
        transport.appEvent({
          connectors_changed: state({ signed_in: { low: true, balance: { credits: 8 } } }),
        }),
      1000,
    );
    return () => clearTimeout(timer);
  }, [transport]);
  return (
    <StoreContext value={store}>
      <Room transport={transport} />
    </StoreContext>
  );
}
