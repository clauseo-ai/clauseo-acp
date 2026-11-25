// The first screen's permissions step's host (issue 5): the desktop's two commands (transport/permission-commands.ts)
// on the desktop, the fake a test or the review page provides for its FakeTransport, and nothing elsewhere: a window
// with neither lists no rows, and the step never shows. System Settings' Files and Folders pane is the transport's own.
import type { FolderChoice } from "@clauseo/components/clauseo/first-launch/folder-choice.tsx";
import { toHostError, type HostTransport } from "../../transport/host-transport.ts";
import {
  askPermissionCommand,
  permissionsCommand,
  setDefaultLocationCommand,
  type OsPermissionState,
  type PermissionId,
  type PermissionRow,
} from "../../transport/permission-commands.ts";

export type {
  OsPermissionState,
  PermissionId,
  PermissionRow,
} from "../../transport/permission-commands.ts";

const KNOWN: readonly string[] = [
  "not_asked",
  "would_ask",
  "asking",
  "granted",
  "denied",
  "blocking",
  "unknown",
] satisfies OsPermissionState[];

/** A state from a newer desktop reads as "unknown", so the step never breaks on it. */
const known = (row: PermissionRow): PermissionRow =>
  KNOWN.includes(row.state) ? row : { ...row, state: "unknown" };

/** The step's host. */
export type PermissionsHost = {
  /** The rows needed on this build; rejects when the desktop couldn't say (a caller keeps what it had). */
  list(): Promise<PermissionRow[]>;
  /** Asks for one row; resolves with the row after, once the lawyer has answered macOS. */
  ask(id: PermissionId): Promise<PermissionRow>;
  /** Picks a folder and makes it where new matters go (set_default_location): its outcome, or null when the lawyer
   * cancels the picker. Absent where nothing can set it (no desktop, no fake), and every button for it is hidden. */
  chooseMattersFolder?: (() => Promise<FolderChoice | null>) | undefined;
  /** Every later list or ask through this window's hosts, with the rows after: a step that stands follows them. */
  watch?:
    | ((
        then: (rows: readonly PermissionRow[], once: boolean, asking: PermissionId | null) => void,
      ) => () => void)
    | undefined;
};

export type { FolderChoice } from "@clauseo/components/clauseo/first-launch/folder-choice.tsx";

/** The picker, then set_default_location with the folder picked. */
async function chooseOver(transport: HostTransport): Promise<FolderChoice | null> {
  const picked = await transport.pickFolder();
  if (picked === null) return null;
  try {
    await setDefaultLocationCommand(transport, picked.folder);
    return { done: picked.display };
  } catch (error) {
    const { code } = toHostError(error);
    return {
      refused:
        code === "settings.locked" || code === "command.invalid" || code === "store.io"
          ? code
          : "other",
    };
  }
}

const provided = new WeakMap<HostTransport, PermissionsHost>();

/** A test's or the review page's fake for `transport`. */
export function providePermissions(transport: HostTransport, host: PermissionsHost): void {
  provided.set(transport, host);
}

const NONE: PermissionsHost = {
  list: () => Promise.resolve([]),
  ask: () => Promise.reject(new Error("no permissions here")),
};

/** The desktop's: its two commands, and the matters-folder choice (set_default_location). */
const desktop = (transport: HostTransport): PermissionsHost => ({
  list: permissionsCommand,
  ask: askPermissionCommand,
  chooseMattersFolder: () => chooseOver(transport),
});

/** The rows the window last heard, from any list or ask through permissionsHostOf: the band's or the first screen's
 * one read, shared with what points at the keychain row (keychain-pointer.tsx), which makes no read of its own. */
type Heard = {
  rows: readonly PermissionRow[] | null;
  /** The keychain was allowed once only (allowedOnce). */
  once: boolean;
  /** The list under way: a second caller shares it, so a focus never stacks reads. */
  listing: Promise<PermissionRow[]> | null;
  /** The ask under way in this window, one at a time per row: every screen shows it waiting (permission-rows.ts's
   * isAsking reads it with the row's own "asking"). */
  asking: { id: PermissionId; answer: Promise<PermissionRow>; heard: boolean } | null;
  subscribe: (then: () => void) => () => void;
  tell: (rows: readonly PermissionRow[]) => void;
  /** Tells every screen again with the rows as they are (an ask started or ended). */
  ring: () => void;
};
const heard = new WeakMap<HostTransport, Heard>();

/** The row this window's ask waits for, until the desktop answers it or a read made since the ask began speaks for it
 * (its own "asking", or the denied a crashed ask leaves): permission-rows.ts's isAsking takes it. */
export const pending = (h: Heard): PermissionId | null =>
  h.asking === null || h.asking.heard ? null : h.asking.id;

export function heardOf(transport: HostTransport): Heard {
  const had = heard.get(transport);
  if (had !== undefined) return had;
  const subs = new Set<() => void>();
  // the lawyer may allow the keychain in another window (Settings › Privacy) or in System Settings: the rows are read
  // again each time this window comes back to the front, once per window while any screen listens, and the listeners
  // go when the last screen does
  const again = () => {
    if (document.visibilityState === "hidden") return;
    permissionsHostOf(transport)
      .list()
      .catch(() => {});
  };
  const made: Heard = {
    rows: null,
    once: false,
    listing: null,
    asking: null,
    subscribe: (then) => {
      if (subs.size === 0 && typeof window !== "undefined") {
        window.addEventListener("focus", again);
        document.addEventListener("visibilitychange", again);
      }
      subs.add(then);
      return () => {
        subs.delete(then);
        if (subs.size > 0 || typeof window === "undefined") return;
        window.removeEventListener("focus", again);
        document.removeEventListener("visibilitychange", again);
      };
    },
    tell: (rows) => {
      made.rows = rows;
      made.ring();
    },
    ring: () => {
      for (const then of subs) then();
    },
  };
  heard.set(transport, made);
  return made;
}

/** The keychain row still would ask after the lawyer answered macOS: a plain Allow let one read through, not Always
 * Allow. The one place that says so, for the band's row and every pointer (UC-1442).
 * Reads the desktop's `asked` (#1344); without it, whether this window's own ask just answered so. */
export const allowedOnce = (row: PermissionRow, justAsked: boolean): boolean =>
  row.id === "keychain" && row.state === "would_ask" && (row.asked ?? justAsked);

/** The step's host for the window's transport. A list that fails rejects: the step keeps the rows it had, and a
 * first read that fails shows nothing. */
export function permissionsHostOf(transport: HostTransport): PermissionsHost {
  const host = provided.get(transport) ?? (transport.kind === "tauri" ? desktop(transport) : NONE);
  const h = heardOf(transport);
  const ask = (id: PermissionId) => {
    if (h.asking?.id === id) return h.asking.answer;
    const answer = host.ask(id).then(
      (bound) => {
        const row = known(bound);
        h.asking = null;
        if (row.id === "keychain") h.once = allowedOnce(row, true);
        h.tell((h.rows ?? [row]).map((r) => (r.id === row.id ? row : r)));
        return row;
      },
      (error: unknown) => {
        h.asking = null;
        h.ring();
        throw error;
      },
    );
    h.asking = { id, answer, heard: false };
    h.ring();
    return answer;
  };
  return {
    list: () => {
      const during = h.asking;
      return (h.listing ??= host
        .list()
        .then((bound) => {
          const rows = bound.map(known);
          // a read begun after this window's ask: the desktop's own word on the row from here on
          if (during !== null && h.asking === during) during.heard = true;
          h.once = rows.some((r) => allowedOnce(r, h.once));
          h.tell(rows);
          return rows;
        })
        .finally(() => (h.listing = null)));
    },
    ask,
    watch: (then) =>
      h.subscribe(() => {
        if (h.rows !== null) then(h.rows, h.once, pending(h));
      }),
    ...(host.chooseMattersFolder === undefined
      ? {}
      : { chooseMattersFolder: host.chooseMattersFolder }),
  };
}

/** The folders Windows' Controlled folder access protects by default, under the lawyer's home. */
const PROTECTED = ["documents", "desktop", "pictures"];

/** A path as the comparison reads it: forward slashes, lower case, no slash at either end, and an absolute path under
 * a home (C:\\Users\\<name>, /Users/<name>, /home/<name>) as the parts below it, as `folder_display` writes it. */
function homeRelative(path: string): string {
  const plain = path
    .replaceAll("\\", "/")
    .toLowerCase()
    .replace(/^\/+|\/+$/g, "");
  return plain.replace(/^(?:[a-z]:\/)?(?:users|home)\/[^/]+\/?/, "");
}

/** The protected folder a matter's folder is in, as the lawyer's computer names it (its first part, home-relative:
 * "Documents", "Desktop", "Pictures"), or null when blockingRow finds no block. */
export function blockedIn(
  rows: readonly PermissionRow[],
  display: string | undefined,
): string | null {
  if (display === undefined || blockingRow(rows, display) === null) return null;
  const parts = display
    .replaceAll("\\", "/")
    .split("/")
    .filter((p) => p !== "");
  const home = parts.findIndex((p, i) => /^(users|home)$/i.test(p) && i <= 1);
  const first = home === -1 ? parts[0] : parts[home + 2];
  return first ?? null;
}

/** The Controlled folder access row that blocks a matter whose folder reads `display` (MatterInfo's folder_display),
 * or null: listed (blocking, or unreadable), and the folder inside the row's folder or a folder it protects by
 * default. */
export function blockingRow(
  rows: readonly PermissionRow[],
  display: string | undefined,
): PermissionRow | null {
  if (display === undefined) return null;
  const row = rows.find(
    (r) => r.id === "controlled_folder_access" && (r.state === "blocking" || r.state === "unknown"),
  );
  if (row === undefined) return null;
  const folder = homeRelative(display);
  const inside = (root: string) =>
    root !== "" && (folder === root || folder.startsWith(`${root}/`));
  const listed = row.folder === null ? "" : homeRelative(row.folder);
  return inside(listed) || PROTECTED.some(inside) ? row : null;
}

/** A fake's matters-folder choice over `transport` (its picker and its dispatch), for tests and the review page. */
export const fakeChoiceOver = (transport: HostTransport) => () => chooseOver(transport);
