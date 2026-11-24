// New matter, from the rail. The rail's New matter opens the new matter form, the same part first launch opens on its
// first screen, in the system's dialog: the matter's name, its client if any, and its folder, chosen in the computer's
// own picker or made new where Clauseo keeps matters. Focus starts on the name. Escape, Cancel or a press on the scrim
// closes it, and focus goes back to New matter, its ring drawn only when a key closed it (D-075). Given onBranch, it
// asks first, new folder or existing folder (new-matter-choice.tsx), with Cancel; the form for the answer then stands
// in the question's place, focus on its name, Back going back to the question.
import { Dialog as Base } from "@base-ui/react/dialog";
import { useState, type RefObject } from "react";
import { focusPlane } from "../ui/dialog-start.ts";
import { dialogPlace, dialogPlane, dialogTitle, scrim } from "../ui/recipes-overlays.ts";
import { cn } from "../lib/cn.ts";
import { useMatterFormWords } from "../lib/words-matter-form.ts";
import { NewMatterChoice } from "./new-matter-choice.tsx";
import { NewMatterForm, type Branch, type NewMatterFormProps } from "./matter-form.tsx";
import { Button } from "../ui/button.tsx";

export function NewMatterDialog({
  open,
  onOpenChange,
  onCreate,
  name,
  client,
  finalFocus,
  branch,
  onBranch,
  last,
  picking,
  ...form
}: Omit<NewMatterFormProps, "onCreate" | "onCancel" | "titled" | "inDialog"> & {
  /** Optional: the question's answer, from the caller: null asks it, "new" or "existing" shows its form. */
  onBranch?: ((branch: Branch | null) => void) | undefined;
  /** Optional: the answer before Back, where focus goes on the question. */
  last?: Branch | null | undefined;
  /** Optional: the picker Existing folder opened is up. */
  picking?: boolean | undefined;
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
          {onBranch !== undefined && branch === undefined ? (
            <div className="matter-form">
              <NewMatterChoice
                onChoose={onBranch}
                last={last}
                picking={picking}
                opening={form.opening}
                testIds={{
                  ...(form.testIds?.made === undefined ? {} : { made: form.testIds.made }),
                  ...(form.testIds?.existing === undefined
                    ? {}
                    : { existing: form.testIds.existing }),
                }}
              />
              <span className="matter-form-actions matter-form-actions-end">
                <Button kind="quiet" onClick={() => onOpenChange(false)}>
                  <span className="deva-nudge">{w("Cancel")}</span>
                </Button>
              </span>
            </div>
          ) : (
            <NewMatterForm
              key={branch ?? ""}
              {...form}
              {...(branch === undefined ? {} : { branch })}
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
              onCancel={() =>
                onBranch !== undefined && branch !== undefined
                  ? onBranch(null)
                  : onOpenChange(false)
              }
            />
          )}
        </Base.Popup>
      </Base.Portal>
    </Base.Root>
  );
}
