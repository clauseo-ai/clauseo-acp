// The window's own controls through Tauri: the caption buttons' minimise, maximise or restore, and close, and whether
// the window is maximised. Only the room build's frame imports this (src/window/), so the default build carries none
// of it. No events: listening to the window (onResized) needs the event plugin's permission, which is not granted;
// the page watches its own resize instead.
import { getCurrentWindow } from "@tauri-apps/api/window";

/** The window, as the caption buttons need it. */
export type WindowControls = {
  minimise(): Promise<void>;
  toggleMaximise(): Promise<void>;
  close(): Promise<void>;
  isMaximised(): Promise<boolean>;
};

/** The current Tauri window's controls. Call only inside the desktop app. */
export function tauriWindowControls(): WindowControls {
  const win = getCurrentWindow();
  return {
    minimise: () => win.minimize(),
    toggleMaximise: () => win.toggleMaximize(),
    close: () => win.close(),
    isMaximised: () => win.isMaximized(),
  };
}
