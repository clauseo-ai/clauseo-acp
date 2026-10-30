import { AlertDialog as Base } from "@base-ui/react/alert-dialog";
import type { ComponentProps, ReactNode } from "react";
import { cn } from "../lib/cn.ts";
import { quietStart, startOf } from "./dialog-start.ts";
import {
  dialogActions,
  dialogBody,
  dialogPlace,
  dialogPlane,
  dialogTitle,
  scrim,
} from "./recipes-overlays.ts";

/** An alert dialog: a dialog that interrupts because something changed under the lawyer's feet, such as a paper
 * edited after Clauseo's edit, and waits for an answer. A press on the scrim doesn't close it; Escape or the safe
 * choice does. Screen readers announce it as an alert. */
export const AlertDialog = Base.Root;
export const AlertDialogTrigger = Base.Trigger;
/** Closes it: render a Button through it, <AlertDialogClose render={<Button kind="quiet" />}>. */
export const AlertDialogClose = Base.Close;

/** The plane itself, over the scrim: the in-hand plane with a card's long shadow, centred, 440 px. The scrim
 * fades in 120 ms; the plane rises 8 px and stops in 220, and leaves in 160. Focus moves into it, stays in it while
 * it's open, and returns to the trigger when it closes. Its starting focus draws no ring until a key is pressed. */
export function AlertDialogPopup({
  title,
  body,
  actions,
  className,
  undoable,
  initialFocus,
  ...props
}: Omit<ComponentProps<typeof Base.Popup>, "className" | "children" | "title"> & {
  /** What's being asked, as a question or a plain statement. */
  title: ReactNode;
  /** What it means, in one or two sentences. */
  body?: ReactNode;
  /** The buttons, the one that acts last. */
  actions: ReactNode;
  className?: string;
  /** Where focus starts, by the app's one rule: the act, the dark primary, when it can be undone; the safe choice,
   * first in the row, when it can't. Unsaid, Base UI's first focusable. An initialFocus given still wins. */
  undoable?: boolean;
}) {
  return (
    <Base.Portal>
      <Base.Backdrop className={scrim} />
      <Base.Popup
        ref={quietStart}
        data-dialog-plane=""
        className={cn(dialogPlace, dialogPlane, className)}
        initialFocus={initialFocus ?? startOf(undoable)}
        {...props}
      >
        <Base.Title className={dialogTitle}>{title}</Base.Title>
        {body !== undefined && <Base.Description className={dialogBody}>{body}</Base.Description>}
        <div className={dialogActions} data-dialog-acts="">
          {actions}
        </div>
      </Base.Popup>
    </Base.Portal>
  );
}
