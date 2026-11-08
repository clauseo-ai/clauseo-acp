// The room with Clauseo's own folder chooser open, for registry-folder-chooser.tsx: New matter, a name, and Change
// over a host whose picker opens the chooser; then, once its first listing has landed, a step of the entry's own.
import { useEffect } from "react";
import type { FakeTransport } from "../transport/fake-transport.ts";
import { NEW_MATTER, type Step } from "./hosts.ts";
import { RoomAfter } from "./states.tsx";

const CHANGE: readonly Step[] = [...NEW_MATTER, { press: "matter-existing" }];

/** The room after Change, and once the chooser's first listing has landed, `then` run on the page once. */
export function Chooser({ host, then }: { host: () => FakeTransport; then?: () => void }) {
  useEffect(() => {
    if (then === undefined) return;
    let done = false;
    const run = () => {
      const choose = document.querySelector<HTMLButtonElement>('[data-testid="chooser-choose"]');
      if (done || choose === null || choose.disabled) return;
      done = true;
      watch.disconnect();
      then();
    };
    const watch = new MutationObserver(run);
    watch.observe(document.body, { subtree: true, childList: true, attributes: true });
    return () => watch.disconnect();
  }, [then]);
  return <RoomAfter host={host} steps={CHANGE} />;
}
