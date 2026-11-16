// ?fullscreen=1 or 0 on the probe page (probe-main.tsx): the screen the window stands on, so the room reads macOS's
// full screen as the app does (components/lib/full-screen.ts, the window's size against the screen's). 1: the screen
// is the window's size, as in full screen; 0: the screen is a little taller than the window, as a window under the
// menu bar. Without it the browser's own screen stands, which a headless one makes 800 by 600, smaller than any
// window, so the probe reads full screen. Use with ?lights=native, the Mac's frame. Not shipped.

/** Sets the screen's size for ?fullscreen=1 or 0, before the room renders; any other value leaves the screen alone. */
export function fakeFullScreen(value: string | null): void {
  if (value !== "1" && value !== "0") return;
  const taller = value === "1" ? 0 : 1;
  Object.defineProperty(window.screen, "width", {
    configurable: true,
    get: () => window.innerWidth,
  });
  Object.defineProperty(window.screen, "height", {
    configurable: true,
    get: () => window.innerHeight + taller,
  });
}
