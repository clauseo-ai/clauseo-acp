import { useState } from "react";
import { InternalWindow } from "./internal/internal-window.tsx";
import { frameBatch, type Batch } from "./store/batch.ts";
import { StoreContext } from "./store/hooks.ts";
import { createViewStore, type ViewStore } from "./store/store.ts";
import type { HostTransport } from "./transport/host-transport.ts";

// The window this build mounts: R12's internal window, or with VITE_CLAUSEO_WINDOW=room the room (src/room/). A
// compile-time constant, so the default build drops the room's import and carries none of its code or CSS; the room
// build loads it before the first render. main.tsx tests the same variable to leave the display to the room.
const room =
  import.meta.env.VITE_CLAUSEO_WINDOW === "room" ? await import("./room/entry.ts") : null;

/** The whole app: the window this build mounts over the transport, with one view store for the page. `batch` says
 * when the store applies what arrived (once a frame, every 100 ms in the room's lite, unless a test says otherwise).
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
    const display = room === null ? null : room.connectRoom(transport);
    const made = createViewStore(transport, {
      batch: display === null ? () => batch ?? frameBatch : () => batch ?? display.batch(),
    });
    onStore?.(made);
    return made;
  });
  // `cargo xtask buildtime web` times hot reload by adding an attribute to this element's opening
  // tag, ROOT_TAG in scripts/buildtime-lib.mjs. When this element changes, change ROOT_TAG with it.
  return (
    <div id="app-root">
      <StoreContext value={store}>
        {room === null ? (
          <InternalWindow transport={transport} />
        ) : (
          <room.Room transport={transport} />
        )}
      </StoreContext>
    </div>
  );
}
