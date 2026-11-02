import { useState } from "react";
import { InternalWindow } from "./internal/internal-window.tsx";
import { frameBatch, type Batch } from "./store/batch.ts";
import { StoreContext } from "./store/hooks.ts";
import { createViewStore, type ViewStore } from "./store/store.ts";
import type { HostTransport } from "./transport/host-transport.ts";

// The window this build mounts: R12's internal window, or in the room build the room (src/room/). A compile-time
// constant, so the default build drops the room's import and carries none of its code or CSS, and the room build drops
// R12's window; the room build loads the room before the first render. The switch is spelt inline here, not imported
// from room-build.ts: Rolldown does not fold an imported constant, so the default build would keep the room's chunk.
// room-build.test.ts holds the spellings together. main.tsx tests room-build.ts's constant to leave the display to the
// room.
const ROOM = import.meta.env.VITE_CLAUSEO_WINDOW === "room";
const room =
  import.meta.env.VITE_CLAUSEO_WINDOW === "room" ? await import("./room/entry.ts") : null;

/** The room, in the room build. */
function RoomWindow({ transport }: { transport: HostTransport }) {
  return room === null ? null : <room.Room transport={transport} />;
}

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
        {/* tested on the constant, so the room build drops R12's window as the default build drops the room */}
        {ROOM ? <RoomWindow transport={transport} /> : <InternalWindow transport={transport} />}
      </StoreContext>
    </div>
  );
}
