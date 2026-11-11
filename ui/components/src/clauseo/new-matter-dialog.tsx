// New matter, from the rail. The rail's New matter opens the new matter form, the same part first launch opens on its
// first screen, in the system's dialog: the matter's name, its client if any, and its folder, chosen in the computer's
// own picker or made new where Clauseo keeps matters. Focus starts on the name. Escape, Cancel or a press on the scrim
// closes it, and focus goes back to New matter, its ring drawn only when a key closed it (D-075).
import { Dialog as Base } from "@base-ui/react/dialog";
import { useState, type RefObject } from "react";
import { focusPlane } from "../ui/dialog-start.ts";
import { dialogPlace, dialogPlane, dialogTitle, scrim } from "../ui/recipes-overlays.ts";
import { cn } from "../lib/cn.ts";
import { useMatterFormWords } from "../lib/words-matter-form.ts";
import { NewMatterForm, type NewMatterFormProps } from "./matter-form.tsx";

export function NewMatterDialog({
  open,
  onOpenChange,
  onCreate,
  name,
  client,
  finalFocus,
  ...form
}: Omit<NewMatterFormProps, "onCreate" | "onCancel" | "titled" | "inDialog"> & {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** Create the matter, pressed with a name and a folder, as the form gives it. Returning nothing closes the dialog at
   * once. Given a promise, Create stands pressed until it settles, and the dialog closes only if it resolves to
   * anything but false; a rejected promise, or false, keeps it open, for the caller's error line. */
  onCreate: NewMatterFormProps["onCreate"];
  /** Optional: where focus goes when it closes, New matter's button. Left out, the element focused before it opened. */
  finalFocus?: RefObject<HTMLElement | null>;
}) {
  const w = useMatterFormWords();
  const [{ mount, field, back }] = useState(focusPlane);
  return (
    <Base.Root open={open} onOpenChange={onOpenChange}>
      <Base.Portal>
        <Base.Backdrop className={scrim} />
        {/* a form: focus starts on its first field, by the app's dialog rule, whether opened by a press or open at load */}
        <Base.Popup
          ref={mount}
          data-dialog-plane=""
          className={cn(dialogPlace, dialogPlane, "new-matter-dialog")}
          initialFocus={field()}
          finalFocus={back(finalFocus)}
        >
          <Base.Title className={cn(dialogTitle, "mb-s4")}>{w("A new matter")}</Base.Title>
          <NewMatterForm
            {...form}
            {...(name === undefined ? {} : { name })}
            {...(client === undefined ? {} : { client })}
            titled={false}
            inDialog
            onCreate={(n, c, made) => {
              const done = onCreate(n, c, made);
              if (!(done instanceof Promise)) {
                onOpenChange(false);
                return;
              }
              // closes only on success; the form keeps Create pressed meanwhile and swallows a rejection
              return done.then((ok) => {
                if (ok !== false) onOpenChange(false);
              });
            }}
            onCancel={() => onOpenChange(false)}
          />
        </Base.Popup>
      </Base.Portal>
    </Base.Root>
  );
}
