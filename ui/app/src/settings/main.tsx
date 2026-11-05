// The settings window's entry (settings.html): its own build entry, so the room's first load carries none of it. The
// display starts before the first render through the same store as the room's (display/connect.ts), and a change here
// reaches the room through the host's settings_changed, or without a host through localStorage's storage event.
import "./settings.css";
import { whenWordsReady } from "@clauseo/components/lib/display.ts";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { connectDisplay } from "../display/connect.ts";
import { pageTransport } from "../transport/tauri-transport.ts";
import { SettingsApp } from "./settings-app.tsx";

const container = document.getElementById("root");
if (container === null) {
  throw new Error("settings.html has no #root element");
}

const transport = pageTransport();
const { store } = connectDisplay(transport);

// a Hindi display renders once its words are in (display.ts's whenWordsReady), an English one at once
whenWordsReady(() =>
  createRoot(container).render(
    <StrictMode>
      <SettingsApp store={store} transport={transport} />
    </StrictMode>,
  ),
);
