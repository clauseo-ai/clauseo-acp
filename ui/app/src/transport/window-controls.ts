// The window's own controls through Tauri: the caption buttons' minimise, maximise or restore, and close, and whether
// the window is maximised. The room's frame imports this (src/window/). No events: listening to the window
// (onResized) needs the event plugin's permission, which is not granted; the page watches its own resize instead.
// Tauri's window module loads on the first call, so the page's first paint does not wait for it.

/** The window, as the caption buttons need it. */
export type WindowControls = {
  minimise(): Promise<void>;
  toggleMaximise(): Promise<void>;
  close(): Promise<void>;
  isMaximised(): Promise<boolean>;
};

/** The current Tauri window's controls. Call only inside the desktop app. */
export function tauriWindowControls(): WindowControls {
  const win = async () => (await import("@tauri-apps/api/window")).getCurrentWindow();
  return {
    minimise: async () => (await win()).minimize(),
    toggleMaximise: async () => (await win()).toggleMaximize(),
    close: async () => (await win()).close(),
    isMaximised: async () => (await win()).isMaximized(),
  };
}
