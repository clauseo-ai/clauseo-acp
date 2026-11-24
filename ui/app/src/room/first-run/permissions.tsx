// The first screen's permissions step's state (issue 5), over its host (permissions-host.ts): the rows read once as
// the first screen opens, the one ask under way, and what each row shows. The step stands only where a row needs the
// lawyer as the screen opens (a macOS row not allowed yet, Windows' Controlled folder access blocking or unreadable);
// once it stands it stays for the screen, its rows' outcomes in place. Asking never holds the rest of the screen: the
// ask runs on while the lawyer answers macOS, the row says so, and the other buttons stay pressable. Windows' row is
// asked by itself as it shows, since its ask opens nothing and only brings back where to allow Clauseo.
import { useEffect, useRef, useState } from "react";
import { PermissionsStep } from "@clauseo/components/clauseo/first-launch/permissions.tsx";
import type { Platform } from "@clauseo/components/lib/display.ts";
import type { PermissionId, PermissionRow, PermissionsHost } from "./permissions-host.ts";
import { stepRows } from "./permission-rows.ts";

/** The step, or nothing where no row needs the lawyer. `onOtherFolder` is the first screen's Open the folder. */
export function Permissions({
  host,
  platform,
  onOtherFolder,
  onSettings,
}: {
  host: PermissionsHost;
  platform: Platform;
  onOtherFolder: () => void;
  /** System Settings' Files and Folders pane (the Mac's). */
  onSettings: () => void;
}) {
  const [rows, setRows] = useState<PermissionRow[] | null>(null);
  const [waiting, setWaiting] = useState<PermissionId | null>(null);
  const [failed, setFailed] = useState<PermissionId | null>(null);
  // the step stands once a read finds a row that needs the lawyer, and stays for the screen
  const [stands, setStands] = useState(false);
  const asking = useRef(false);
  const live = useRef(true);
  useEffect(() => {
    live.current = true;
    return () => {
      live.current = false;
    };
  }, []);
  /** The row after an ask, in place. */
  const settle = (row: PermissionRow) =>
    setRows((now) => (now ?? []).map((r) => (r.id === row.id ? row : r)));
  const read = () =>
    void host.list().then((listed) => {
      if (!live.current) return;
      setRows(listed);
      if (listed.some((r) => r.state !== "granted")) setStands(true);
      // Windows' row: its ask brings back where to allow Clauseo, and opens nothing
      for (const row of listed)
        if (row.id === "controlled_folder_access" && row.state !== "granted")
          host.ask(row.id).then(
            (after) => {
              if (live.current) settle(after);
            },
            () => {},
          );
    });
  useEffect(read, [host]);
  if (!stands || rows === null || (platform !== "mac" && platform !== "windows")) return null;
  const ask = (id: PermissionId) => {
    // one host call per press, and none while an ask waits for the lawyer's answer
    if (asking.current) return;
    asking.current = true;
    setWaiting(id);
    setFailed(null);
    host.ask(id).then(
      (after) => {
        asking.current = false;
        if (!live.current) return;
        setWaiting(null);
        settle(after);
      },
      () => {
        asking.current = false;
        if (!live.current) return;
        setWaiting(null);
        setFailed(id);
      },
    );
  };
  const shown = stepRows(rows, waiting, failed);
  if (shown.length === 0) return null;
  const mac = platform === "mac";
  return (
    <PermissionsStep
      rows={shown}
      platform={mac ? "mac" : "windows"}
      onAsk={ask}
      onSettings={mac ? onSettings : undefined}
      onCheck={read}
      onOtherFolder={mac ? undefined : onOtherFolder}
      onChooseFolder={mac ? undefined : host.chooseMattersFolder}
    />
  );
}
