// The Smart App Control note, once (use-sac-note.ts decides when): a quiet status line at the top of the work column,
// never a dialog. One line: if Windows ever blocks Clauseo, turn Smart App Control off. "How to do this" opens the
// install guide's Smart App Control section, with the steps, through the host's link confirm; "Got it" puts it away
// for good. Its own chunk, loaded only when it shows; "Got it" is New matter's synced-folder notice's words, Hindi and
// all.
import { Say } from "@clauseo/components/clauseo/say.tsx";
import { IconAbout } from "@clauseo/components/icons/icons.tsx";
import { useDisplay } from "@clauseo/components/lib/display.ts";
import { rt } from "@clauseo/components/lib/words-room.ts";
import { sc } from "@clauseo/components/lib/words-scan.ts";
import { Button } from "@clauseo/components/ui/button.tsx";
import type { HostTransport } from "../../transport/host-transport.ts";
import { quietly } from "../open.ts";
import { SAC_GUIDE } from "./guide.ts";

export function SacNote({ transport, onGotIt }: { transport: HostTransport; onGotIt: () => void }) {
  const { lang } = useDisplay();
  return (
    <div className="mx-auto box-content max-w-measure px-thread-pad pt-s4" data-testid="sac-note">
      <div className="flex items-center gap-s2 text-small text-muted" role="status">
        <span className="flex-none" aria-hidden="true">
          <IconAbout size={16} />
        </span>
        <p className="min-w-0 flex-1">
          <Say>{rt(lang, "If Windows ever blocks Clauseo, turn off Smart App Control.")}</Say>{" "}
          <Button
            kind="link"
            size="small"
            className="min-h-0 px-0 py-0 text-primary align-baseline underline underline-offset-2"
            data-testid="sac-note-how"
            onClick={() => void quietly("open_external", transport.openExternal(SAC_GUIDE))}
          >
            {rt(lang, "How to do this")}
          </Button>
        </p>
        <Button kind="quiet" size="small" data-testid="sac-note-got-it" onClick={onGotIt}>
          <span className="deva-nudge">{sc(lang, "Got it")}</span>
        </Button>
      </div>
    </div>
  );
}
