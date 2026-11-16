// UD2's scan cells: the room, New matter opened, a name typed and Change pressed, each step once what it acts on shows.
// Apart from registry-ud2.tsx, which exports entries, not components.
import { useEffect, useRef, useState } from "react";
import { Room } from "../room/room.tsx";
import type { FakeTransport } from "../transport/fake-transport.ts";

/** New matter, the name typed, and Change: the folder picked, its scan's lines under the folder line. Each step waits
 * for what it acts on (the form's chunk can land after a fixed wait), once, though Strict Mode runs effects twice. */
export function RoomPicking({ host }: { host: () => FakeTransport }) {
  const [transport] = useState(host);
  const done = useRef(0);
  useEffect(() => {
    const steps: (() => boolean)[] = [
      () => click('[data-testid="new-matter"]'),
      () => {
        const field = document.querySelector<HTMLInputElement>('[data-testid="matter-name"]');
        if (field === null) return false;
        Reflect.set(HTMLInputElement.prototype, "value", "Rathi Steels v. Acme Traders", field);
        field.dispatchEvent(new Event("input", { bubbles: true }));
        return true;
      },
      () => click('[data-testid="matter-existing"]'),
    ];
    const tick = setInterval(() => {
      const step = steps[done.current];
      if (step === undefined) clearInterval(tick);
      else if (step()) done.current += 1;
    }, 100);
    return () => clearInterval(tick);
  }, []);
  return <Room transport={transport} />;
}
const click = (selector: string) => {
  const target = document.querySelector<HTMLElement>(selector);
  target?.click();
  return target !== null;
};
