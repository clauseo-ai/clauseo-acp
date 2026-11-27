// The permissions step where a lawyer who already has matters lands (issue 5, #1303): a band at the head of the room's
// work, over the open matter or the room with none open, whenever the desktop lists a row not asked yet (a new build's
// keychain read, the Documents folder), or a keychain row refused at an earlier start (permission-rows.ts's
// awaitsAllow, the rule every keychain pointer follows too). The same rows, words and acts as the first screen's step
// (first-run/permissions.tsx), headed "Before you go on". A refused keychain row has "Not now", which puts that row away
// in this window until the next start (in memory only, never stored): the band stays while another row still wants its
// Allow, and goes once none does; the other screens still point at the keychain's Allow. Never a wall: the room under it stays usable, and the band
// stays where it is as the lawyer moves between matters. It stays while a row waits or was refused, so a refused row
// can be allowed from it (or from Settings › Privacy), and leaves once every row is allowed.
// RC A4: the band takes its own slot at the head of the work (flex-none, in flow), before .room-work, the box the
// thread and the layers over it (the pinned question) are placed in: it pushes them down and never covers them. The
// desktop lists Documents not asked again after every update (its answers are kept per build, store.rs); on a Mac that
// answered it before (documents-answered.ts) that row alone raises no band, only DocumentsAgain's one line with Allow.
import { useEffect, useState } from "react";
import { DocumentsAgain } from "@clauseo/components/clauseo/first-launch/permissions.tsx";
import { useDisplay, type Platform } from "@clauseo/components/lib/display.ts";
import type { HostTransport } from "../transport/host-transport.ts";
import { documentsAnswered } from "./first-run/documents-answered.ts";
import { awaitsAllow, isAsking, quietDocuments } from "./first-run/permission-rows.ts";
import {
  permissionsHostOf,
  type PermissionRow,
  type PermissionsHost,
} from "./first-run/permissions-host.ts";
import { Permissions } from "./first-run/permissions.tsx";

/** The windows whose band's keychain row the lawyer put away (Not now): this run's only, so it stands again at the next
 * start. */
const away = new WeakSet<HostTransport>();

/** The full band stands for these rows: one awaits its Allow, the quiet Documents row aside. */
const fullFor = (rows: readonly PermissionRow[], answered: boolean) =>
  rows.some((r) => awaitsAllow(r) && !quietDocuments(r, answered));

export function PermissionsBand({ transport }: { transport: HostTransport }) {
  const { platform } = useDisplay();
  const [host] = useState(() => permissionsHostOf(transport));
  const [gone, setGone] = useState(() => away.has(transport));
  // read once as the band mounts, as the step's own first read: an answer heard later in this run leaves it as it is
  const [answered] = useState(documentsAnswered);
  return (
    <>
      <Full
        host={host}
        transport={transport}
        platform={platform}
        answered={answered}
        gone={gone}
        onGone={() => {
          away.add(transport);
          setGone(true);
        }}
      />
      {platform === "mac" && answered && <Quiet host={host} />}
    </>
  );
}

/** The quiet line, while the full band didn't stand at the first read and the Documents row is listed not asked. */
function Quiet({ host }: { host: PermissionsHost }) {
  const [rows, setRows] = useState<readonly PermissionRow[] | null>(null);
  const [first, setFirst] = useState<readonly PermissionRow[] | null>(null);
  const [asking, setAsking] = useState<PermissionRow["id"] | null>(null);
  const [failed, setFailed] = useState(false);
  useEffect(() => {
    let live = true;
    const take = (listed: readonly PermissionRow[], waiting: PermissionRow["id"] | null) => {
      if (!live) return;
      setRows(listed);
      setFirst((had) => had ?? listed);
      setAsking(waiting);
    };
    const stop = host.watch?.((listed, _once, waiting) => take(listed, waiting));
    host.list().then(
      (listed) => take(listed, null),
      () => {},
    );
    return () => {
      live = false;
      stop?.();
    };
  }, [host]);
  if (first === null || rows === null || fullFor(first, true)) return null;
  const docs = rows.find((r) => quietDocuments(r, true));
  if (docs === undefined) return null;
  return (
    <div className={SLOT} data-testid="permissions-band">
      <DocumentsAgain
        waiting={isAsking(docs, asking)}
        failed={failed}
        onAsk={() => {
          setFailed(false);
          host.ask("files_documents").then(
            () => {},
            () => setFailed(true),
          );
        }}
      />
    </div>
  );
}

/** In the reading column, as the thread's lines stand: the measure, centred, the thread's side padding; its own slot in
 * the work's column (flex-none), never over the thread. */
const SLOT =
  "flex-none mx-auto box-content max-w-measure px-thread-pad pt-s4 max-mid:mx-0 max-mid:max-w-none max-mid:px-s4";

function Full({
  host,
  transport,
  platform,
  answered,
  gone,
  onGone,
}: {
  host: PermissionsHost;
  transport: HostTransport;
  platform: Platform;
  answered: boolean;
  gone: boolean;
  onGone: () => void;
}) {
  return (
    <Permissions
      host={host}
      platform={platform}
      heading="Before you go on"
      opens={(listed) => fullFor(listed, answered)}
      leavesWhenAllowed
      putAway={gone ? "keychain" : undefined}
      onNotNow={onGone}
      onSettings={() => void transport.openPrivacySettings().catch(() => {})}
      wrap={(step) => (
        <div className={SLOT} data-testid="permissions-band">
          {step}
        </div>
      )}
    />
  );
}
