// Whether the window fills its screen, as macOS's own full screen does: the page then stands where the traffic lights
// stood, and macOS shows them only as the pointer reaches the top. The Mac's window grants the page no window
// commands (capabilities/frame.json is Windows and Linux only), so the page reads its own size against the screen's.
import { useSyncExternalStore } from "react";

const fills = () =>
  typeof window !== "undefined" &&
  window.innerWidth >= window.screen.width &&
  window.innerHeight >= window.screen.height;

const subscribe = (changed: () => void) => {
  window.addEventListener("resize", changed);
  return () => window.removeEventListener("resize", changed);
};

const never = () => () => {};
const no = () => false;

/** Whether the window fills its screen, watched only where `watch` is set (the Mac's own lights): elsewhere false. */
export function useFullScreen(watch: boolean): boolean {
  return useSyncExternalStore(watch ? subscribe : never, watch ? fills : no, no);
}
