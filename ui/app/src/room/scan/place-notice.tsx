// A synced folder's notice under New matter's folder line (UD2): "This folder syncs with OneDrive, so changes to its
// papers sync too, including Clauseo's. Versions are kept on this computer, outside the folder." with "Got it". It sits
// above Create, and Create's press reads it as Got it does (folder-check.tsx). A notice, not an error: a status, so
// New matter's error line stays its one alert. Part of New matter's chunk, with the scan's lines.
import { Say } from "@clauseo/components/clauseo/say.tsx";
import { IconAbout } from "@clauseo/components/icons/icons.tsx";
import { sc } from "@clauseo/components/lib/words-scan.ts";
import { useWords } from "@clauseo/components/lib/words.ts";
import { Button } from "@clauseo/components/ui/button.tsx";
import type { RefObject } from "react";

export function PlaceNotice({
  service,
  gotItRef,
  onGotIt,
}: {
  service: string;
  /** Where focus goes when the notice only shows on Create's press: the Got it button (focusGotIt). */
  gotItRef?: RefObject<HTMLButtonElement | null> | undefined;
  onGotIt: () => void;
}) {
  const { lang } = useWords();
  return (
    <div className="scan-place" role="status" data-testid="place-notice" data-place="synced">
      <span className="scan-place-icon" aria-hidden="true">
        <IconAbout size={16} />
      </span>
      <p className="scan-place-text">
        <Say>
          {sc(
            lang,
            "This folder syncs with {service}, so changes to its papers sync too, including Clauseo's. Versions are kept on this computer, outside the folder.",
            { service },
          )}
        </Say>
      </p>
      <Button
        ref={gotItRef}
        kind="quiet"
        size="small"
        data-testid="synced-got-it"
        onClick={onGotIt}
      >
        <span className="deva-nudge">{sc(lang, "Got it")}</span>
      </Button>
    </div>
  );
}
