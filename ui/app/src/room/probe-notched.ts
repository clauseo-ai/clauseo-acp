// ?notched=1 on the probe page (probe-main.tsx): a notched MacBook in macOS's full screen. macOS puts the full-screen
// window below the camera band, so the window is 32 px shorter than its screen and the room's own size read says
// "windowed"; the desktop says full screen on the app feed (`window_full_screen`, transport/full-screen-feed.ts), and
// that word stands in for the size. So the screen here is the window's width and 32 px taller, and the fake transport
// plays `window_full_screen: true`. Use with ?lights=native, the Mac's frame. Not shipped.
import type { FakeTransport } from "../transport/fake-transport.ts";

/** The camera band's height a notched MacBook's full-screen window stands below. */
const BAND = 32;

/** For ?notched=1, before the room renders: the screen 32 px taller than the window, and the desktop's word that the
 * window is in full screen. Any other value leaves both alone. */
export function fakeNotched(value: string | null, transport: FakeTransport): void {
  if (value !== "1") return;
  Object.defineProperty(window.screen, "width", {
    configurable: true,
    get: () => window.innerWidth,
  });
  Object.defineProperty(window.screen, "height", {
    configurable: true,
    get: () => window.innerHeight + BAND,
  });
  transport.fullScreenChanged(true);
}
