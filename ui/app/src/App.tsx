import { useState } from "react";
import { connectRoom, Room } from "./room/entry.ts";
import type { Batch } from "./store/batch.ts";
import { StoreContext } from "./store/hooks.ts";
import { createViewStore, type ViewStore } from "./store/store.ts";
import type { HostTransport } from "./transport/host-transport.ts";
import { useNetworkSignal } from "./transport/network.ts";
import type { WindowControls } from "./transport/window-controls.ts";

/** The whole app: the room (src/room/) over the transport, with one view store for the page. `batch` says when the
 * store applies what arrived (once a frame, every 100 ms in the room's lite, unless a test says otherwise).
 * `onStore` hears the store once it is made: only the perf build (src/perf/) passes it. `windowControls` stands in for
 * Tauri's window: only the probe passes it, to draw the Mac's native lights' frame. */
export function App({
  transport,
  batch,
  onStore,
  windowControls,
}: {
  transport: HostTransport;
  batch?: Batch;
  onStore?: ((store: ViewStore) => void) | undefined;
  windowControls?: WindowControls;
}) {
  const [store] = useState(() => {
    // the room's display settings start before its first render; in lite its batch is every 100 ms
    const display = connectRoom(transport);
    const made = createViewStore(transport, { batch: () => batch ?? display.batch() });
    onStore?.(made);
    return made;
  });
  // the webview's network signal to the host (set_network), once for the page
  useNetworkSignal(transport);
  // `cargo xtask buildtime web` times hot reload by adding an attribute to this element's opening
  // tag, ROOT_TAG in scripts/buildtime-lib.mjs. When this element changes, change ROOT_TAG with it.
  return (
    <div id="app-root">
      <StoreContext value={store}>
        <Room transport={transport} {...(windowControls === undefined ? {} : { windowControls })} />
      </StoreContext>
    </div>
  );
}
