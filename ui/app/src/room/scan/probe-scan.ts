// ?scan=small, empty or large: the probe's picker gives a folder and its scan plays these counts, so New matter's folder
// line can be seen for a small folder (200 files, 6 KB), an empty one, or one of 1.5 MB. ?scan=long-path: the picker
// gives a folder with a long path, and the host's place for new matters is long too, so New matter's folder row and
// its "Use a new folder in" line can be seen cut short in their middle. Fake data only.
import type { FakeHost } from "../../transport/fake-transport.ts";
import type { FolderScanned } from "../../transport/protocol.ts";

const COUNTS: Record<string, { files: number; bytes: number }> = {
  small: { files: 200, bytes: 6_000 },
  empty: { files: 0, bytes: 0 },
  large: { files: 24, bytes: 1_500_000 },
  "long-path": { files: 200, bytes: 6_000 },
};

const SHORT = "/Users/rm/Matters/Rathi Steels";
/** ?scan=long-path's folder, as the desktop writes one under home (no `~`). */
const LONG =
  "Documents/Clauseo/Matters/2026/Meridian Agro Traders Pvt Ltd v Kestrel Finance Ltd/Correspondence and pleadings/Interim";
/** ?scan=long-path's place for new matters, as the desktop writes it. */
const LONG_PLACE =
  "Documents/Clauseo/Matters/2026/Commercial disputes before the Bombay High Court/Original side/New matters";

/** The host's place for new matters on `?scan=`'s page: a long one for long-path, else the probe's usual one. */
export const scanLocation = (value: string | null): string =>
  value === "long-path" ? LONG_PLACE : "Documents/Clauseo";

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
  const display = value === "long-path" ? LONG : SHORT;
  return {
    pickFolder: () => ({ folder: "probe-folder", display }),
    scan: (folder) => [done(folder)],
  };
}
