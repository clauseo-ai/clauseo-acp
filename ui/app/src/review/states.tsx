// The states the registered strings show in: the real room (src/room/room.tsx) over a host scripted for the state, and
// the room after the steps a state takes once rendered (New matter, a name, Change, Create).
import { useEffect, useRef, useState } from "react";
import { Room } from "../room/room.tsx";
import type { FakeTransport } from "../transport/fake-transport.ts";
import type { Step } from "./hosts.ts";

/** The room over the transport `host` builds, built once for the frame's life. */
export function RoomOver({ host }: { host: () => FakeTransport }) {
  const [transport] = useState(host);
  return <Room transport={transport} />;
}

/** The room over `host`, then `steps` taken in order as the lawyer would, each once the last has settled. */
export function RoomAfter({ host, steps }: { host: () => FakeTransport; steps: readonly Step[] }) {
  const [transport] = useState(host);
  const taken = useRef(false);
  useEffect(() => {
    // once, though Strict Mode runs the effect twice
    if (taken.current) return;
    taken.current = true;
    const settle = () => new Promise((resolve) => setTimeout(resolve, 50));
    void (async () => {
      await settle();
      for (const step of steps) {
        if ("press" in step) {
          document.querySelector<HTMLElement>(`[data-testid="${step.press}"]`)?.click();
        } else {
          const field =
            document.querySelectorAll<HTMLInputElement>('[role="dialog"] input')[step.type.field];
          if (field !== undefined) {
            Reflect.set(HTMLInputElement.prototype, "value", step.type.text, field);
            field.dispatchEvent(new Event("input", { bubbles: true }));
          }
        }
        await settle();
      }
    })();
  }, [steps]);
  return <Room transport={transport} />;
}
