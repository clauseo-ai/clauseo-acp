// One session of Clauseo's own folder chooser (transport/chooser.ts), loaded the first time the desktop asks for it:
// it lists the first place to start, then each place the lawyer goes, through the session's calls, and ends the
// request with the folder the desktop registered after its native confirm, or with Cancel. A confirm the lawyer
// declines leaves the chooser open where it was. A call the desktop refuses (a typed location it doesn't take, a
// session it closed) says Clauseo can't use the folder, and the chooser stays.
import { FolderChooser } from "@clauseo/components/clauseo/folder-chooser.tsx";
import { useEffect, useRef, useState } from "react";
import type { ChooserRequest } from "../transport/chooser.ts";
import type { Go, Listing, StartPoint } from "../transport/protocol.ts";
import "./folder-chooser.css";

/** Where the chooser opens: the last place, else home, else the Desktop, else the first drive. */
function firstOf(starts: readonly StartPoint[]): StartPoint | undefined {
  for (const kind of ["last_pick", "home", "desktop"] as const) {
    const found = starts.find((s) => s.kind === kind);
    if (found !== undefined) return found;
  }
  return starts[0];
}

/** The folder shown as it was, saying Clauseo can't use where the lawyer tried to go. */
const refused = (shown: Listing | null): Listing => ({
  display: shown?.display ?? "",
  crumbs: shown?.crumbs ?? [],
  state: { kind: "not_allowed" },
});

export function FolderChooserSession({ request }: { request: ChooserRequest }) {
  const [listing, setListing] = useState<Listing | null>(null);
  // busy from the start: the first listing is on its way
  const [busy, setBusy] = useState(true);
  const [waiting, setWaiting] = useState(false);
  const live = useRef(true);
  const started = useRef(false);
  useEffect(() => {
    live.current = true;
    return () => {
      live.current = false;
    };
  }, []);

  /** Lists `where`, busy already. */
  const load = async (where: Go) => {
    try {
      const next = await request.wire.list(where);
      if (live.current) setListing(next);
    } catch {
      if (live.current) setListing(refused);
    } finally {
      if (live.current) setBusy(false);
    }
  };

  const go = (where: Go) => {
    setBusy(true);
    void load(where);
  };

  useEffect(() => {
    // once, though Strict Mode runs the effect twice
    if (started.current) return;
    started.current = true;
    const first = firstOf(request.start.starts);
    void load(first === undefined ? { kind: "again" } : { kind: "start", key: first.key });
  });

  const choose = async () => {
    setWaiting(true);
    try {
      const picked = await request.wire.choose();
      if (picked !== null) request.chosen(picked);
    } catch {
      if (live.current) setListing(refused);
    } finally {
      if (live.current) setWaiting(false);
    }
  };

  return (
    <FolderChooser
      open
      starts={request.start.starts}
      listing={listing}
      busy={busy}
      waiting={waiting}
      onGo={go}
      onChoose={() => void choose()}
      onCancel={() => request.cancel()}
    />
  );
}
