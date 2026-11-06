// UD1's cells for switching accounts (0048, #493) and the gate's refusal line, apart from registry-ud1.tsx to keep that
// file under the size limit. A refusal needs the lawyer's hand: the room over a fake host that refuses the scene's
// setup commands, then the scene's steps taken in order, each once the last has shown (a press, a key typed, an event
// the host plays).
import { useEffect, useRef, useState } from "react";
import type { FakeTransport } from "../transport/fake-transport.ts";
import type { SetupEvent } from "../transport/protocol.ts";
import { RoomOver } from "./states.tsx";

/** One step: a control pressed (a CSS selector), words typed into the gate's field, or a setup event played. */
export type DrivenStep = { press: string } | { type: string } | { play: SetupEvent };

/** The room over `host`, then `steps` in order, each once what it acts on shows. */
export function RoomDriven({
  host,
  steps,
}: {
  host: () => FakeTransport;
  steps: readonly DrivenStep[];
}) {
  const [transport] = useState(host);
  // how far along, kept across Strict Mode's second run, so no step is taken twice
  const done = useRef(0);
  useEffect(() => {
    const take = () => {
      const step = steps[done.current];
      if (step === undefined) return;
      if ("play" in step) {
        done.current += 1;
        transport.setupEvent(step.play);
        return;
      }
      if ("type" in step) {
        const field = document.querySelector<HTMLInputElement>(".fl-gate input");
        if (field === null) return;
        done.current += 1;
        Reflect.set(HTMLInputElement.prototype, "value", step.type, field);
        field.dispatchEvent(new Event("input", { bubbles: true }));
        return;
      }
      const target = document.querySelector<HTMLElement>(step.press);
      if (target === null || (target as HTMLButtonElement).disabled) return;
      done.current += 1;
      target.click();
    };
    const tick = setInterval(take, 100);
    const watch = new MutationObserver(take);
    watch.observe(document.body, { subtree: true, childList: true });
    take();
    return () => {
      clearInterval(tick);
      watch.disconnect();
    };
  }, [steps, transport]);
  return <RoomOver host={() => transport} />;
}
