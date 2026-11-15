// A matter's kept permissions, from the rail's matter menu ("Kept permissions…"): each rule
// "Always for this matter" kept for Claude's permission requests, one row in plain words
// ("Run: npm test", "Edit: work/out.txt", "Any page on example.com") with a Remove that sends
// `remove_kept_permission` and reads the list again; Claude then asks again before it. An empty
// list says so in one line; a refusal says its code's words. Close, Escape or the scrim closes it.
import { useDisplay } from "@clauseo/components/lib/display.ts";
import { errorWords } from "@clauseo/components/lib/words-errors.ts";
import { kp } from "@clauseo/components/lib/words-kept.ts";
import { say } from "@clauseo/components/lib/words-ui.ts";
import { Button } from "@clauseo/components/ui/button.tsx";
import { Dialog, DialogClose, DialogPopup } from "@clauseo/components/ui/dialog.tsx";
import { settingLabel, settingNote } from "@clauseo/components/ui/recipes-controls.ts";
import type { HostTransport } from "../transport/host-transport.ts";
import type { MatterId } from "../transport/protocol.ts";
import { keptLine, useKeptPermissions } from "./kept-permissions-commands.ts";

/** The list, mounted only while it is open, so each opening reads it afresh. */
function KeptList({ transport, matter }: { transport: HostTransport; matter: MatterId }) {
  const { lang } = useDisplay();
  const { kept, failed, removing, remove } = useKeptPermissions(transport, matter);
  return (
    <div className="mt-3 flex max-h-80 flex-col gap-2 overflow-y-auto">
      {kept !== null && kept.length === 0 && (
        <p className={settingNote} data-testid="kept-permissions-empty">
          {say(kp(lang, "Nothing is kept for this matter. Claude asks each time."))}
        </p>
      )}
      {kept !== null && kept.length > 0 && (
        <ul className="flex flex-col gap-2">
          {kept.map((rule) => {
            const line = keptLine(lang, rule);
            return (
              <li
                key={rule.rule}
                className="flex items-center justify-between gap-3"
                data-testid="kept-permission"
              >
                <p className={`${settingLabel} min-w-0 break-words`}>{say(line)}</p>
                <Button
                  kind="quiet"
                  size="small"
                  data-testid="kept-permission-remove"
                  aria-label={kp(lang, "Remove {permission}", { permission: line })}
                  disabled={removing !== null}
                  onClick={() => remove(rule.rule)}
                >
                  {kp(lang, "Remove")}
                </Button>
              </li>
            );
          })}
        </ul>
      )}
      {failed !== null && (
        <p className={settingNote} role="status" data-testid="kept-permissions-failed">
          {say(errorWords(lang, failed))}
        </p>
      )}
    </div>
  );
}

export function KeptPermissions({
  transport,
  matter,
  onClose,
}: {
  transport: HostTransport;
  /** The matter whose list is open; null while closed. */
  matter: MatterId | null;
  onClose: () => void;
}) {
  const { lang } = useDisplay();
  return (
    <Dialog open={matter !== null} onOpenChange={(open) => !open && onClose()}>
      <DialogPopup
        data-testid="kept-permissions"
        title={say(kp(lang, "Kept permissions"))}
        body={say(
          kp(
            lang,
            "Claude doesn't ask before these in this matter: you chose “Always for this matter”.",
          ),
        )}
        between={matter !== null && <KeptList transport={transport} matter={matter} />}
        actions={
          <DialogClose render={<Button kind="quiet" data-testid="kept-permissions-close" />}>
            {kp(lang, "Close")}
          </DialogClose>
        }
      />
    </Dialog>
  );
}
