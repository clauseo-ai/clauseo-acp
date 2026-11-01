// The caption buttons wired to the window: its maximised state, read on mount and after each resize, and the three
// presses. Every call to the window is caught and logged, never thrown into React.
import { useEffect, useState } from "react";
import type { WindowControls } from "../transport/window-controls.ts";

/** What CaptionButtons takes from the window. */
export type WindowFrame = {
  maximised: boolean;
  onMinimise: () => void;
  onToggleMaximise: () => void;
  onClose: () => void;
};

/** One call to the window, a throw before its promise included, as a promise. */
const settle = <T>(call: () => Promise<T>): Promise<T> =>
  new Promise<T>((resolve) => resolve(call()));

/** Runs one call to the window, logging a failure. */
function attempt(what: string, call: () => Promise<unknown>): void {
  settle(call).catch((error: unknown) => console.warn(`window ${what} failed: ${String(error)}`));
}

/** The window's frame state and presses, ready to spread into CaptionButtons. Maximising or restoring resizes the
 * page, so a resize re-reads the state: at most once per animation frame, and a reply after unmount is dropped. */
export function useWindowFrame(controls: WindowControls): WindowFrame {
  const [maximised, setMaximised] = useState(false);
  useEffect(() => {
    let live = true;
    let frame: number | null = null;
    const read = () => {
      frame = null;
      settle(() => controls.isMaximised()).then(
        (m) => {
          if (live) setMaximised(m);
        },
        (error: unknown) => console.warn(`window isMaximised failed: ${String(error)}`),
      );
    };
    const onResize = () => {
      frame ??= requestAnimationFrame(read);
    };
    read();
    window.addEventListener("resize", onResize);
    return () => {
      live = false;
      window.removeEventListener("resize", onResize);
      if (frame !== null) cancelAnimationFrame(frame);
    };
  }, [controls]);
  return {
    maximised,
    onMinimise: () => attempt("minimise", () => controls.minimise()),
    onToggleMaximise: () => attempt("toggleMaximise", () => controls.toggleMaximise()),
    onClose: () => attempt("close", () => controls.close()),
  };
}
