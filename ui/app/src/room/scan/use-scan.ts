// The scan of a folder picked in New matter (R20a, ADR 0032): one `scan_folder` per existing folder picked, its progress
// from the host's `folder_scanned` events for that folder alone (any other folder's are dropped), and what the form
// needs of it: the counts and the file reached, where the folder lives, a refusal by its code, and whether the lawyer
// has said "Got it" to a synced folder's notice. Create waits for the first event (it carries `place`) at most as long
// as the folder check, and a scan still running when Create goes on is cancelled first, so the import's walk doesn't run
// beside it. Stop cancels the scan and drops the pick. Part of New matter's chunk (folder-check.tsx). No timers but the
// one wait Create makes; nothing polls.
import { useEffect, useRef, useState } from "react";
import { toHostError } from "../../transport/host-transport.ts";
import type {
  FolderRefId,
  FolderScanned,
  HostError,
  SyncService,
} from "../../transport/protocol.ts";
import type { Matters } from "../matters.ts";

/** How long Create waits for the scan's first event: the folder check's own wait (folder-check.tsx's CHECK_WAIT_MS). */
export const PLACE_WAIT_MS = 5000;

export type Scan = {
  folder: FolderRefId;
  /** The latest event for this folder; null before the first. */
  latest: FolderScanned | null;
  /** `scan_folder` refused with a `matter.folder_*` code: said as Create would say it. */
  refused: HostError | null;
  /** "Got it" pressed on the synced notice, for this adding. */
  gotIt: boolean;
  /** `cancel_scan` sent already, as Create went on, or `scan_folder` failed for another reason: nothing to wait for. */
  stopped: boolean;
};

/** Each service by its owner's name; "other" has none, so no synced notice, and cloud-only files are kept "online".
 * A service this version doesn't know reads as other. */
const SERVICES: Partial<Record<SyncService, string>> = {
  one_drive: "OneDrive",
  google_drive: "Google Drive",
  icloud: "iCloud Drive",
  dropbox: "Dropbox",
  box: "Box",
};

export function serviceOf(scanned: FolderScanned | null): string | null {
  const place = scanned?.place;
  if (place === undefined || typeof place !== "object" || place.synced === undefined) return null;
  return SERVICES[place.synced.service] ?? null;
}

/** Whether the folder is synced by a service with a name to show: the notice shows, and Create waits for "Got it". */
export const isSynced = (scanned: FolderScanned | null): boolean => serviceOf(scanned) !== null;

/** The scan of the picked folder, for the form. */
export function useScan(matters: Matters) {
  const [scan, setScan] = useState<Scan | null>(null);
  // what the event listener and Create read now, without a render between
  const now = useRef<Scan | null>(null);
  const firstHeard = useRef<(() => void) | null>(null);
  const put = (next: Scan | null) => {
    now.current = next;
    setScan(next);
  };
  useEffect(
    () =>
      matters.onScan((scanned) => {
        const at = now.current;
        if (at === null || scanned.folder !== at.folder) return;
        put({ ...at, latest: scanned });
        firstHeard.current?.();
        firstHeard.current = null;
      }),
    [matters],
  );
  const running = (s: Scan | null): s is Scan =>
    s !== null && !s.stopped && s.refused === null && s.latest?.done !== true;
  /** A folder picked: its scan starts, and an earlier one's events are dropped (the host cancels it itself). */
  const start = (folder: FolderRefId) => {
    // without the app feed no event could say how it goes: no scan
    if (matters.next.app === null) return;
    put({ folder, latest: null, refused: null, gotIt: false, stopped: false });
    matters.scan(folder).catch((thrown: unknown) => {
      const error = toHostError(thrown);
      const at = now.current;
      if (at === null || at.folder !== folder) return;
      // refused: said as Create would say it; any other failure leaves no scan to wait for
      if (error.code.startsWith("matter.folder_")) put({ ...at, refused: error });
      else {
        console.warn(`scan_folder failed: ${JSON.stringify(error.code)}`);
        put({ ...at, stopped: true });
      }
      firstHeard.current?.();
      firstHeard.current = null;
    });
  };
  /** The pick dropped (Stop, a close, a made matter): a scan still running is cancelled. */
  const clear = () => {
    const at = now.current;
    if (running(at)) matters.cancelScan(at.folder);
    firstHeard.current = null;
    put(null);
  };
  const gotIt = () => {
    const at = now.current;
    if (at !== null) put({ ...at, gotIt: true });
  };
  /** Create's turn: waits for the first event at most PLACE_WAIT_MS, then says whether Create may go on. It may not
   * while a synced folder's notice waits for "Got it" (the notice shows by then). Going on, a running scan is
   * cancelled first. */
  const settle = async (): Promise<boolean> => {
    const at = now.current;
    if (at === null) return true;
    if (at.latest === null && at.refused === null && !at.stopped) {
      let timer: ReturnType<typeof setTimeout> | undefined;
      await new Promise<void>((resolve) => {
        firstHeard.current = resolve;
        timer = setTimeout(resolve, PLACE_WAIT_MS);
      });
      clearTimeout(timer);
    }
    const after = now.current;
    if (after === null || after.folder !== at.folder) return false;
    if (isSynced(after.latest) && !after.gotIt) return false;
    if (running(after)) {
      matters.cancelScan(after.folder);
      put({ ...after, stopped: true });
    }
    return true;
  };
  return { scan, start, clear, gotIt, settle };
}
