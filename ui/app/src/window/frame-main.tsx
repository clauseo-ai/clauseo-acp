// The dev-only stand-in for the room's window frame until the room lands: frame/index.html loads it (Tauri's devUrl
// pointed at http://localhost:1420/frame/), and Vite's build never does. Deleted with frame-page.tsx when the room
// lands.
import "./frame.css";
import { DEFAULT_DISPLAY, showDisplay, startDisplay } from "@clauseo/components/lib/display.ts";
import { PartsRoot } from "@clauseo/components/ui/root.tsx";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { hostPlatform } from "../platform.ts";
import { pageTransport } from "../transport/tauri-transport.ts";
import { tauriWindowControls } from "../transport/window-controls.ts";
import { FramePage } from "./frame-page.tsx";

const container = document.getElementById("root");
if (container === null) throw new Error("frame/index.html has no #root element");

const transport = pageTransport();
startDisplay(DEFAULT_DISPLAY);
// The host answers after the first render: showDisplay, unlike startDisplay, tells useDisplay's readers.
hostPlatform(transport).then(
  (platform) => showDisplay({ ...DEFAULT_DISPLAY, platform }),
  (error: unknown) => console.warn(`display_info failed: ${JSON.stringify(error)}`),
);
// Outside the desktop app there is no window to call: the caption buttons log what they would do.
const controls = transport.kind === "null" ? null : tauriWindowControls();

createRoot(container).render(
  <StrictMode>
    <PartsRoot>
      <FramePage controls={controls} />
    </PartsRoot>
  </StrictMode>,
);
