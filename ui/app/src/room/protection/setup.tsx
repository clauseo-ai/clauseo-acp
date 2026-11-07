// UC: a set-up press, as the room's note, the firm's notice and Settings share it. Where it brings
// Windows' administrator prompt (view.ts's asksAdmin: R17's retry_asks_admin, or its fallback),
// only a full pool's Add room goes on, through the "Unknown publisher" confirm first; design B's
// app never raises the prompt for a set-up, so such a press sends nothing (its callers show the
// repair, blocked.ts's needsInstaller, in the button's place). Otherwise it goes at once. While it
// runs, the waiting line stands in the button's place ("Choose Yes in the window Windows shows."
// behind the prompt, "Setting up the Safe workspace." without), and the host's in_progress keeps it
// after a reload; a refusal leaves its code's words.
import { ProtectionConfirm } from "@clauseo/components/clauseo/protection-confirm.tsx";
import { useState, type ReactNode } from "react";
import type { ProtectionState } from "../../store/protection.ts";
import type { Cause } from "./causes.ts";
import { asksAdmin } from "./view.ts";
import { busyOf } from "./words.ts";

export { failedWords, waitingWords } from "./words.ts";

/** A set-up press: `press` opens Add room's confirm where an administrator is asked, else sends at
 * once; `confirm` is the dialog to place beside the button. */
export function useSetUpPress(
  state: ProtectionState,
  cause: Cause | null,
  send: () => void,
): { press: () => void; confirm: ReactNode } {
  const [open, setOpen] = useState(false);
  const press = () => {
    if (busyOf(state) !== null) return;
    if (!asksAdmin(state.view, cause)) send();
    else if (cause?.group === "pool") setOpen(true);
  };
  return {
    press,
    confirm: (
      <ProtectionConfirm
        act="add"
        open={open}
        onOpenChange={setOpen}
        onConfirm={() => {
          setOpen(false);
          send();
        }}
      />
    ),
  };
}
