// R18, "Use files from another matter": the other matters, from the rail's list, each a button;
// each name sanitised and isolated (isolate.ts); a pick closes the list and the room asks the host (menu.tsx), which opens its own picker at that
// matter's folder and then its dialog. Nothing is granted here. Loaded on the menu's press.
import { useDisplay } from "@clauseo/components/lib/display.ts";
import { isolate } from "@clauseo/components/lib/isolate.ts";
import { aw } from "@clauseo/components/lib/words-allowances.ts";
import { say } from "@clauseo/components/lib/words-ui.ts";
import { Button } from "@clauseo/components/ui/button.tsx";
import { Dialog, DialogClose, DialogPopup } from "@clauseo/components/ui/dialog.tsx";
import type { MatterId } from "../../transport/protocol.ts";
import type { MatterInfo } from "../host-next.ts";

export function OtherMatterDialog({
  matters,
  onPick,
  onClose,
}: {
  /** The other matters, in the rail's order: one at least. */
  matters: readonly MatterInfo[];
  onPick: (matter: MatterId) => void;
  onClose: () => void;
}) {
  const { lang } = useDisplay();
  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogPopup
        data-testid="other-matter"
        title={aw(lang, "Use files from another matter")}
        body={
          <span className="grid gap-2">
            <span className="block">
              {say(
                aw(
                  lang,
                  "Choose the matter. Clauseo then opens its folder so you can pick the files, which it can only read.",
                ),
              )}
            </span>
            {/* the menu offers this list only while there is another matter */}
            <span role="list" className="grid gap-1">
              {matters.map((m) => (
                <span role="listitem" key={m.matter} className="block">
                  <Button
                    kind="quiet"
                    className="w-full justify-start"
                    data-testid="other-matter-pick"
                    onClick={() => onPick(m.matter)}
                  >
                    <bdi lang="en">{isolate(m.name)}</bdi>
                  </Button>
                </span>
              ))}
            </span>
          </span>
        }
        actions={
          <DialogClose render={<Button kind="quiet" />}>
            <span className="deva-nudge">{aw(lang, "Cancel")}</span>
          </DialogClose>
        }
      />
    </Dialog>
  );
}
