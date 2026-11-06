// A synced folder's notice under New matter's folder line (UD2): "This folder syncs with OneDrive, so changes to its
// papers sync too, including Clauseo's. Versions are kept on this computer, outside the folder." with "Got it". Create
// waits for "Got it", once per adding (use-scan.ts). A notice, not an error: a status, so New matter's error line
// stays its one alert. Part of New matter's chunk, with the scan's lines.
import { Say } from "@clauseo/components/clauseo/say.tsx";
import { IconAbout } from "@clauseo/components/icons/icons.tsx";
import { sc } from "@clauseo/components/lib/words-scan.ts";
import { useWords } from "@clauseo/components/lib/words.ts";
import { Button } from "@clauseo/components/ui/button.tsx";

export function PlaceNotice({ service, onGotIt }: { service: string; onGotIt: () => void }) {
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
      <Button kind="quiet" size="small" data-testid="synced-got-it" onClick={onGotIt}>
        <span className="deva-nudge">{sc(lang, "Got it")}</span>
      </Button>
    </div>
  );
}
