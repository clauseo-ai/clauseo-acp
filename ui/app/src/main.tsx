import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { DEFAULT_DISPLAY, startDisplay } from "@clauseo/components/lib/display.ts";
import { App } from "./App.tsx";
import { hostPlatform } from "./platform.ts";
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

const transport = pageTransport();
// The window's chrome follows the OS the host reports; nothing on the page waits for it.
hostPlatform(transport).then(
  (platform) => startDisplay({ ...DEFAULT_DISPLAY, platform }),
  (error: unknown) => console.warn(`display_info failed: ${JSON.stringify(error)}`),
);

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
