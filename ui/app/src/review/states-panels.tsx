// A state for the panel toggles' entries (registry-uc-panels.tsx): the room with one toggle pressed. The press keeps the
// choice for the window (layout-store.ts), and every review cell shares the page's storage, so the choice is let go in
// the same task as the press, before another cell can read it.
import { useEffect, useRef } from "react";
import type { FakeTransport } from "../transport/fake-transport.ts";
import { RoomOver } from "./states.tsx";

/** The room, its toggle `press` (a test id) pressed once it shows, and the window's kept layout let go at once. */
export function RoomPanelHidden({ host, press }: { host: () => FakeTransport; press: string }) {
  const box = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const el = box.current;
    if (el === null) return;
    let pressed = false;
    const tap = () => {
      const target = el.querySelector<HTMLElement>(`[data-testid="${press}"]`);
      if (pressed || target === null) return;
      pressed = true;
      watch.disconnect();
      target.click();
      localStorage.removeItem("clauseo.layout.main");
    };
    const watch = new MutationObserver(tap);
    watch.observe(el, { subtree: true, childList: true });
    tap();
    return () => watch.disconnect();
  }, [press]);
  return (
    <div ref={box} className="contents">
      <RoomOver host={host} />
    </div>
  );
}
