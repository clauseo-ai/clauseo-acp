// The permissions step where a lawyer who already has matters lands (issue 5, #1303): a band at the head of the room's
// work, over the open matter or the room with none open, whenever the desktop lists a row not asked yet (a new build's
// keychain read, the Documents folder), or a keychain row refused at an earlier start (permission-rows.ts's
// awaitsAllow, the rule every keychain pointer follows too). The same rows, words and acts as the first screen's step
// (first-run/permissions.tsx), headed "Before you go on". A refused keychain row has "Not now", which puts the band away
// in this window until the next start (in memory only, never stored); the other screens still point at its Allow. Never a wall: the room under it stays usable, and the band
// stays where it is as the lawyer moves between matters. It stays while a row waits or was refused, so a refused row
// can be allowed from it (or from Settings › Privacy), and leaves once every row is allowed.
import { useState } from "react";
import { useDisplay } from "@clauseo/components/lib/display.ts";
import type { HostTransport } from "../transport/host-transport.ts";
import { awaitsAllow } from "./first-run/permission-rows.ts";
import { permissionsHostOf } from "./first-run/permissions-host.ts";
import { Permissions } from "./first-run/permissions.tsx";

/** The windows whose band the lawyer put away (Not now): this run's only, so it stands again at the next start. */
const away = new WeakSet<HostTransport>();

export function PermissionsBand({ transport }: { transport: HostTransport }) {
  const { platform } = useDisplay();
  const [host] = useState(() => permissionsHostOf(transport));
  const [gone, setGone] = useState(() => away.has(transport));
  if (gone) return null;
  return (
    <Permissions
      host={host}
      platform={platform}
      heading="Before you go on"
      opens={(listed) => listed.some(awaitsAllow)}
      leavesWhenAllowed
      onNotNow={() => {
        away.add(transport);
        setGone(true);
      }}
      onSettings={() => void transport.openPrivacySettings().catch(() => {})}
      wrap={(step) => (
        // in the reading column, as the thread's lines stand: the measure, centred, the thread's side padding
        <div
          className="mx-auto box-content max-w-measure px-thread-pad pt-s4 max-mid:mx-0 max-mid:max-w-none max-mid:px-s4"
          data-testid="permissions-band"
        >
          {step}
        </div>
      )}
    />
  );
}
