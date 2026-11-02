import { useState } from "react";
import { connectRoom, Room } from "./room/entry.ts";
import type { Batch } from "./store/batch.ts";
import { StoreContext } from "./store/hooks.ts";
import { createViewStore, type ViewStore } from "./store/store.ts";
import type { HostTransport } from "./transport/host-transport.ts";

/** The whole app: the room (src/room/) over the transport, with one view store for the page. `batch` says when the
 * store applies what arrived (once a frame, every 100 ms in the room's lite, unless a test says otherwise).
 * `onStore` hears the store once it is made: only the perf build (src/perf/) passes it. */
export function App({
  transport,
  batch,
  onStore,
}: {
  transport: HostTransport;
  batch?: Batch;
  onStore?: ((store: ViewStore) => void) | undefined;
}) {
  const [store] = useState(() => {
    // the room's display settings start before its first render; in lite its batch is every 100 ms
    const display = connectRoom(transport);
    const made = createViewStore(transport, { batch: () => batch ?? display.batch() });
    onStore?.(made);
    return made;
  });
  // `cargo xtask buildtime web` times hot reload by adding an attribute to this element's opening
  // tag, ROOT_TAG in scripts/buildtime-lib.mjs. When this element changes, change ROOT_TAG with it.
  return (
    <div id="app-root">
      <StoreContext value={store}>
        <Room transport={transport} />
      </StoreContext>
    </div>
  );
}
