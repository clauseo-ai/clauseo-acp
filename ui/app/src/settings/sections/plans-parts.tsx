// Plans' small shared pieces (UD2): a press that asks first, in an AlertDialog, and the line a refused setup command
// leaves under its block, in its code's words.
import type { GateRefusal } from "@clauseo/components/clauseo/first-launch/gate.tsx";
import { useDisplay } from "@clauseo/components/lib/display.ts";
import { errorWords } from "@clauseo/components/lib/words-errors.ts";
import { fl } from "@clauseo/components/lib/words-first-launch.ts";
import { say, ui } from "@clauseo/components/lib/words-ui.ts";
import {
  AlertDialog,
  AlertDialogClose,
  AlertDialogPopup,
  AlertDialogTrigger,
} from "@clauseo/components/ui/alert-dialog.tsx";
import { Button } from "@clauseo/components/ui/button.tsx";

/** A quiet button that asks before it acts: the dialog's title and body, Cancel first (the safe choice, where focus
 * starts, since none of these can be undone from here), then the act under the button's own words. */
export function Confirm({
  label,
  title,
  body,
  onConfirm,
  testId,
}: {
  label: string;
  title: string;
  body: string;
  onConfirm: () => void;
  testId: string;
}) {
  const { lang } = useDisplay();
  return (
    <AlertDialog>
      <AlertDialogTrigger render={<Button kind="quiet" size="small" data-testid={testId} />}>
        {say(label)}
      </AlertDialogTrigger>
      <AlertDialogPopup
        title={say(title)}
        body={say(body)}
        undoable={false}
        actions={
          <>
            <AlertDialogClose render={<Button kind="quiet" data-testid={`${testId}-cancel`} />}>
              {ui(lang, "Cancel")}
            </AlertDialogClose>
            <AlertDialogClose
              render={<Button kind="tone" data-testid={`${testId}-confirm`} />}
              onClick={onConfirm}
            >
              {say(label)}
            </AlertDialogClose>
          </>
        }
      />
    </AlertDialog>
  );
}

/** The last setup command desktop refused for the plan, in its code's words; a `command.conflict` while the plan is
 * switching accounts says the switch isn't done (#503's line). */
export function Refused({ refused }: { refused: GateRefusal | null }) {
  const { lang } = useDisplay();
  if (refused === null) return null;
  return (
    <p className="set-plan-refused" role="status" data-testid="plans-refused">
      {say(
        refused.switching && refused.code === "command.conflict"
          ? fl(lang, "Clauseo is still switching accounts. Try again in a moment.")
          : errorWords(lang, refused.code),
      )}
    </p>
  );
}
