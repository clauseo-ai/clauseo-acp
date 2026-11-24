// The permissions step's state (issue 5), over its host (permissions-host.ts): the rows read once as it mounts, the one
// ask under way, and what each row shows. Where the lawyer lands it stands: the first screen (a row not allowed yet),
// the band over the room for a lawyer with matters (a row not asked yet, permissions-band.tsx), Settings › Privacy (any
// row). Once it stands it stays, its rows' outcomes in place; a re-read that fails (Check again) keeps the rows it had.
// Asking never holds the rest of the screen: the ask runs on while the lawyer answers macOS, the row says so, and the
// other buttons stay pressable. Windows' row is asked by itself as it shows, since its ask opens nothing and only
// brings back where to allow Clauseo.
import { useEffect, useRef, useState, type ReactNode } from "react";
import {
  PermissionsStep,
  type StepHeading,
} from "@clauseo/components/clauseo/first-launch/permissions.tsx";
import type { Platform } from "@clauseo/components/lib/display.ts";
import type { PermissionId, PermissionRow, PermissionsHost } from "./permissions-host.ts";
import { stepRows } from "./permission-rows.ts";

/** The step, or nothing where no row needs the lawyer. `onOtherFolder` is the first screen's Open the folder. */
export function Permissions({
  host,
  platform,
  onOtherFolder,
  onSettings,
  heading = "Before your first matter",
  opens = (listed) => listed.some((r) => r.state !== "granted"),
  wrap = (step) => step,
  leavesWhenAllowed = false,
}: {
  host: PermissionsHost;
  platform: Platform;
  /** The first screen's Open the folder (Windows); none in Settings, where there is no first matter. */
  onOtherFolder?: (() => void) | undefined;
  /** Its heading; null in Settings, whose Permissions group names it. */
  heading?: StepHeading | null;
  /** Whether the first read's rows make it stand: a row not allowed yet, unless the caller says otherwise. */
  opens?: (listed: readonly PermissionRow[]) => boolean;
  /** The band's: it leaves once every row is allowed, none waiting and none refused. */
  leavesWhenAllowed?: boolean;
  /** What the step stands in, drawn only while it shows (Settings' group). */
  wrap?: (step: ReactNode) => ReactNode;
  /** System Settings' Files and Folders pane (the Mac's). */
  onSettings: () => void;
}) {
  const [rows, setRows] = useState<PermissionRow[] | null>(null);
  const [waiting, setWaiting] = useState<PermissionId | null>(null);
  const [failed, setFailed] = useState<PermissionId | null>(null);
  // the first read's rows: the step stands when they open it (`opens`), and stays
  const [first, setFirst] = useState<PermissionRow[] | null>(null);
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
    void host.list().then(
      (listed) => {
        if (!live.current) return;
        setRows(listed);
        setFirst((had) => had ?? listed);
        // Windows' row: its ask brings back where to allow Clauseo, and opens nothing
        for (const row of listed)
          if (row.id === "controlled_folder_access" && row.state !== "granted")
            host.ask(row.id).then(
              (after) => {
                if (live.current) settle(after);
              },
              () => {},
            );
      },
      () => {},
    );
  useEffect(read, [host]);
  const stands = first !== null && opens(first);
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
  if (leavesWhenAllowed && shown.every((r) => r.shows === "granted")) return null;
  const mac = platform === "mac";
  return wrap(
    <PermissionsStep
      rows={shown}
      platform={mac ? "mac" : "windows"}
      heading={heading}
      onAsk={ask}
      onSettings={mac ? onSettings : undefined}
      onCheck={read}
      onOtherFolder={mac ? undefined : onOtherFolder}
      onChooseFolder={mac ? undefined : host.chooseMattersFolder}
    />,
  );
}
