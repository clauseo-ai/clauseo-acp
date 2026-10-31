import { useState } from "react";
import { InternalWindow } from "./internal/internal-window.tsx";
import { frameBatch, type Batch } from "./store/batch.ts";
import { StoreContext } from "./store/hooks.ts";
import { createViewStore, type ViewStore } from "./store/store.ts";
import type { HostTransport } from "./transport/host-transport.ts";

/** The whole app, for now: the plain internal window over the transport, with one view store for the page. `batch`
 * says when the store applies what arrived (once a frame unless a test says otherwise). `onStore` hears the store once
 * it is made: only the perf build (src/perf/) passes it. */
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
    const made = createViewStore(transport, { batch: () => batch ?? frameBatch });
    onStore?.(made);
    return made;
  });
  // `cargo xtask buildtime web` times hot reload by adding an attribute to this element's opening
  // tag, ROOT_TAG in scripts/buildtime-lib.mjs. When this element changes, change ROOT_TAG with it.
  return (
    <div id="app-root">
      <StoreContext value={store}>
        <InternalWindow transport={transport} />
      </StoreContext>
    </div>
  );
}
