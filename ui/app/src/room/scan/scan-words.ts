// The scan's lines as words (UD2): the counts and the cloud-only line, pure, for scan-lines.tsx and its tests.
import type { Lang } from "@clauseo/components/lib/display.ts";
import { sc } from "@clauseo/components/lib/words-scan.ts";
import type { FolderScanned } from "../../transport/protocol.ts";
import { serviceOf } from "./use-scan.ts";

/** A count in Indian grouping, as money.ts's count() says it: kept here so New matter's chunk shares no chunk with the
 * thread's, which holds money.ts (vite-config.test.ts). */
const COUNT = new Intl.NumberFormat("en-IN-u-nu-latn", { maximumFractionDigits: 0 });
const count = (n: number): string => COUNT.format(n);

/** A size's number in `lang`'s grouping, Latin digits as the counts take, with at most `digits` decimals. */
const sizes = new Map<string, Intl.NumberFormat>();
function sized(lang: Lang, n: number, digits: number): string {
  const key = `${lang}-${digits}`;
  let f = sizes.get(key);
  if (f === undefined) {
    f = new Intl.NumberFormat(`${lang}-IN-u-nu-latn`, { maximumFractionDigits: digits });
    sizes.set(key, f);
  }
  return f.format(n);
}

/** Where a folder's size says MB, not KB: from what would round to 1,000 KB. */
const MB_FROM = 999_500;

/** A folder's size, D-072: KB under 1 MB, whole and at least 1 for any bytes at all ("6"); MB from there, to one
 * decimal under 10 MB ("1.5") and whole above. Decimal units, as the computer's own file sizes are. */
export function sizeOf(lang: Lang, bytes: number): { unit: "KB" | "MB"; n: string } {
  if (bytes < MB_FROM)
    return {
      unit: "KB",
      n: sized(lang, bytes <= 0 ? 0 : Math.max(1, Math.round(bytes / 1_000)), 0),
    };
  const mb = bytes / 1_000_000;
  return { unit: "MB", n: sized(lang, mb, mb < 10 ? 1 : 0) };
}

/** The counts so far: UD2-01, or UD2-02 for one file; UC-135 and UC-136 in KB under 1 MB. A finished scan that found no
 * file says so (UC-137), and a scan that has found none yet says nothing: never "0 files, 0 MB". */
export function countsLine(lang: Lang, s: FolderScanned): string | null {
  if (s.files <= 0) return s.done ? sc(lang, "The folder is empty.") : null;
  const size = sizeOf(lang, s.bytes);
  if (size.unit === "KB")
    return s.files === 1
      ? sc(lang, "1 file, {kb} KB", { kb: size.n })
      : sc(lang, "{n} files, {kb} KB", { n: count(s.files), kb: size.n });
  return s.files === 1
    ? sc(lang, "1 file, {mb} MB", { mb: size.n })
    : sc(lang, "{n} files, {mb} MB", { n: count(s.files), mb: size.n });
}

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
