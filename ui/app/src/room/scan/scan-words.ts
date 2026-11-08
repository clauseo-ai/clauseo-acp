// The scan's lines as words (UD2): the counts and the cloud-only line, pure, for scan-lines.tsx and its tests.
import type { Lang } from "@clauseo/components/lib/display.ts";
import { sc } from "@clauseo/components/lib/words-scan.ts";
import type { FolderScanned } from "../../transport/protocol.ts";
import { serviceOf } from "./use-scan.ts";

/** A count in Indian grouping, as money.ts's count() says it: kept here so New matter's chunk shares no chunk with the
 * thread's, which holds money.ts (vite-config.test.ts). */
const COUNT = new Intl.NumberFormat("en-IN-u-nu-latn", { maximumFractionDigits: 0 });
const count = (n: number): string => COUNT.format(n);

/** Bytes as whole MB: rounded, at least 1 for any bytes at all, grouped as counts are. */
export const mbOf = (bytes: number): string =>
  count(bytes <= 0 ? 0 : Math.max(1, Math.round(bytes / 1_000_000)));

/** The counts so far: UD2-01, or UD2-02 for one file. */
export const countsLine = (lang: Lang, s: FolderScanned): string =>
  s.files === 1
    ? sc(lang, "1 file, {mb} MB", { mb: mbOf(s.bytes) })
    : sc(lang, "{n} files, {mb} MB", { n: count(s.files), mb: mbOf(s.bytes) });

/** The cloud-only line: by the service's name where the folder is synced, else "online"; null with none. */
export function cloudOnlyLine(lang: Lang, s: FolderScanned): string | null {
  if (s.cloud_only <= 0) return null;
  const service = serviceOf(s);
  const one = s.cloud_only === 1;
  if (service !== null)
    return one
      ? sc(lang, "1 file is kept only in {service} and downloads as it's read.", { service })
      : sc(lang, "{n} files are kept only in {service} and download as they're read.", {
          n: count(s.cloud_only),
          service,
        });
  return one
    ? sc(lang, "1 file is kept only online and downloads as it's read.")
    : sc(lang, "{n} files are kept only online and download as they're read.", {
        n: count(s.cloud_only),
      });
}
