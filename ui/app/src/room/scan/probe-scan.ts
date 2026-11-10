// ?scan=small, empty or large: the probe's picker gives a folder and its scan plays these counts, so New matter's folder
// line can be seen for a small folder (200 files, 6 KB), an empty one, or one of 1.5 MB. Fake data only.
import type { FakeHost } from "../../transport/fake-transport.ts";
import type { FolderScanned } from "../../transport/protocol.ts";

const COUNTS: Record<string, { files: number; bytes: number }> = {
  small: { files: 200, bytes: 6_000 },
  empty: { files: 0, bytes: 0 },
  large: { files: 24, bytes: 1_500_000 },
};

/** The picker and the scan for `?scan=`'s value, or none for any other. */
export function scanProbe(value: string | null): Pick<FakeHost, "pickFolder" | "scan"> | null {
  const counts = value === null ? undefined : COUNTS[value];
  if (counts === undefined) return null;
  const done = (folder: string): FolderScanned => ({
    folder,
    ...counts,
    place: "local",
    cloud_only: 0,
    done: true,
    cancelled: false,
  });
  return {
    pickFolder: () => ({ folder: "probe-folder", display: "/Users/rm/Matters/Rathi Steels" }),
    scan: (folder) => [done(folder)],
  };
}
