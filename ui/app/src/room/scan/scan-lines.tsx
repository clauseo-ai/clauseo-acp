// The scan's lines under New matter's folder line (UD2): "24 files, 38 MB", then, while the walk runs, "Reading the
// folder. Now Board minutes.docx." with a quiet Stop; the reading line goes once the scan is done. Then the folder's
// place: a synced folder's notice with "Got it" (place-notice.tsx), and files kept only online. A network folder adds
// nothing here: U2's warning says it (folder-warning.tsx). No end estimate. Part of New matter's chunk.
import "./scan.css";
import { Say } from "@clauseo/components/clauseo/say.tsx";
import { fileName } from "@clauseo/components/lib/words-core.ts";
import { sc } from "@clauseo/components/lib/words-scan.ts";
import { t, useWords } from "@clauseo/components/lib/words.ts";
import { Button } from "@clauseo/components/ui/button.tsx";
import { PlaceNotice } from "./place-notice.tsx";
import { cloudOnlyLine, countsLine } from "./scan-words.ts";
import { serviceOf, type Scan } from "./use-scan.ts";

/** The lines for one scan; nothing before its first event, after a refusal (the form's error line says it), or once
 * cancelled. */
export function ScanLines({
  scan,
  onStop,
  onGotIt,
}: {
  scan: Scan;
  onStop: () => void;
  onGotIt: () => void;
}) {
  const { lang } = useWords();
  const s = scan.latest;
  if (s === null || scan.refused !== null || s.cancelled) return null;
  const reading = s.done
    ? null
    : s.now_reading === undefined || s.now_reading === ""
      ? sc(lang, "Reading the folder.")
      : sc(lang, "Reading the folder. Now {title}.", { title: fileName(s.now_reading) });
  const cloud = cloudOnlyLine(lang, s);
  const service = serviceOf(s);
  return (
    <div className="scan text-small" data-testid="scan" data-done={s.done ? "" : undefined}>
      <p className="scan-line" role="status">
        <span className="scan-counts">
          <Say>{countsLine(lang, s)}</Say>
        </span>
        {reading !== null && (
          <>
            {" "}
            <span className="scan-reading">
              <Say>{reading}</Say>
            </span>
          </>
        )}
        {reading !== null && (
          <Button
            kind="quiet"
            size="small"
            className="scan-stop"
            data-testid="scan-stop"
            onClick={onStop}
          >
            <span className="deva-nudge">{t(lang, "Stop")}</span>
          </Button>
        )}
      </p>
      {service !== null && !scan.gotIt && <PlaceNotice service={service} onGotIt={onGotIt} />}
      {cloud !== null && (
        <p className="scan-line" data-testid="scan-cloud-only">
          <Say>{cloud}</Say>
        </p>
      )}
    </div>
  );
}
