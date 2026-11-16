// Where Clauseo's own folder chooser shows: mounted once in the room, it shows the session its transport's chooser
// holds (transport/chooser.ts), loading the chooser the first time one opens. Every pick goes through it unchanged:
// New matter's Change and Create, the first screen's Open the folder, and a moved folder's Locate.
import { late } from "@clauseo/components/lib/late.ts";
import { lazy, Suspense, useEffect, useSyncExternalStore } from "react";
import { chooserOf } from "../transport/chooser.ts";
import type { HostTransport } from "../transport/host-transport.ts";

const Session = lazy(() =>
  late(import("./folder-chooser.tsx")).then((m) => ({ default: m.FolderChooserSession })),
);

export function FolderChooserHost({ transport }: { transport: HostTransport }) {
  const store = chooserOf(transport);
  useEffect(() => store.host(), [store]);
  const request = useSyncExternalStore(store.subscribe, store.current, store.current);
  if (request === null) return null;
  return (
    <Suspense fallback={null}>
      <Session key={request.start.chooser} request={request} />
    </Suspense>
  );
}
