import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { DEFAULT_DISPLAY, startDisplay } from "@clauseo/components/lib/display.ts";
import { App } from "./App.tsx";
import { hostPlatform } from "./platform.ts";
import { pageTransport } from "./transport/tauri-transport.ts";

const container = document.getElementById("root");
if (container === null) {
  throw new Error("index.html has no #root element");
}

const transport = pageTransport();
// The window's chrome follows the OS the host reports; nothing on the page waits for it.
hostPlatform(transport).then(
  (platform) => startDisplay({ ...DEFAULT_DISPLAY, platform }),
  (error: unknown) => console.warn(`display_info failed: ${JSON.stringify(error)}`),
);

createRoot(container).render(
  <StrictMode>
    <App transport={transport} />
  </StrictMode>,
);
