// The confirm before an unsafe-site allow (0053), its own chunk (vite.config.ts's
// PRELOADED_CHUNKS), loaded on a notice's first press (site-blocked.tsx): the site, why the list
// names it, and how far the allow reaches. Cancel first, where focus starts (an allow is taken back
// only in Settings), then Allow. Confirming calls onConfirm once; Cancel, Escape or closing sends
// nothing.
import type { Lang } from "@clauseo/components/lib/display.ts";
import { bl } from "@clauseo/components/lib/words-blocklist.ts";
import { say, ui } from "@clauseo/components/lib/words-ui.ts";
import {
  AlertDialog,
  AlertDialogClose,
  AlertDialogPopup,
} from "@clauseo/components/ui/alert-dialog.tsx";
import { Button } from "@clauseo/components/ui/button.tsx";
import type { AllowScope } from "../../transport/protocol.ts";

/** The confirm before an allow: the site, why the list names it, and how far the allow reaches. */
export function AllowConfirm({
  lang,
  host,
  reason,
  scope,
  open,
  onOpenChange,
  onConfirm,
}: {
  lang: Lang;
  host: string;
  reason: string | null;
  scope: AllowScope;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onConfirm: () => void;
}) {
  const always = scope === "always";
  return (
    <AlertDialog open={open} onOpenChange={onOpenChange}>
      <AlertDialogPopup
        data-testid="site-allow-confirm"
        data-scope={scope}
        undoable={false}
        title={say(
          always
            ? bl(lang, "Always allow {host}?", { host })
            : bl(lang, "Allow {host} for this matter?", { host }),
        )}
        body={
          <span className="np-note-body">
            {reason !== null && <span>{say(reason)}</span>}
            <span>
              {say(
                always
                  ? bl(lang, "From now on, Clauseo can open this site in every matter.")
                  : bl(lang, "From now on, Clauseo can open this site in this matter."),
              )}{" "}
              {say(bl(lang, "You can take this back in Settings, under Privacy."))}
            </span>
          </span>
        }
        actions={
          <>
            <AlertDialogClose render={<Button kind="quiet" data-testid="site-allow-cancel" />}>
              {ui(lang, "Cancel")}
            </AlertDialogClose>
            <AlertDialogClose
              render={<Button kind="tone" data-testid="site-allow-ok" />}
              onClick={onConfirm}
            >
              {bl(lang, "Allow")}
            </AlertDialogClose>
          </>
        }
      />
    </AlertDialog>
  );
}
