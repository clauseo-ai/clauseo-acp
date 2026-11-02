import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App.tsx";
import { setTimedHook } from "./transport/channel.ts";
import { pageTransport } from "./transport/tauri-transport.ts";

const container = document.getElementById("root");
if (container === null) {
  throw new Error("index.html has no #root element");
}

// The e2e and marks builds' init script sets `__CLAUSEO_PERF__` before any page script (R13 contract §3); only then
// does the perf module load, as its own chunk. Until it takes over, timed channel messages are unwrapped and not timed.
const perf = window.__CLAUSEO_PERF__ === undefined ? null : import("./perf/index.ts");
if (perf !== null) setTimedHook(() => {});

// The room shows its own display settings, the platform the host reports included (src/room/start.ts).
const transport = pageTransport();

createRoot(container).render(
  <StrictMode>
    <App
      transport={transport}
      onStore={
        perf === null ? undefined : (store) => void perf.then((m) => m.start(transport, store))
      }
    />
  </StrictMode>,
);
