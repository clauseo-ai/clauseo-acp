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

/** Whether the window fills its screen, watched only where `watch` is set (the Mac's own lights): elsewhere false.
 * `given` is the host's word where it has one, and stands in for the size; the size is read only without it. */
export function useFullScreen(watch: boolean, given?: boolean): boolean {
  const read = watch && given === undefined;
  const size = useSyncExternalStore(read ? subscribe : never, read ? fills : no, no);
  return watch && given !== undefined ? given : size;
}
