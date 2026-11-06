// UC: a set-up press, as the room's note, the firm's notice and Settings share it. Where the set-up brings Windows'
// administrator prompt (view.ts's asksAdmin: R17's retry_asks_admin, or its fallback), the press opens the "Unknown
// publisher" confirm first and the command goes only from there; otherwise it goes at once. While it runs, the
// waiting line stands in the button's place ("Choose Yes in the window Windows shows." behind the prompt, "Setting up
// the Safe workspace." without), and the host's in_progress keeps it after a reload; a refusal leaves its code's words.
import { ProtectionConfirm } from "@clauseo/components/clauseo/protection-confirm.tsx";
import { useState, type ReactNode } from "react";
import type { ProtectionState } from "../../store/protection.ts";
import type { Cause } from "./causes.ts";
import { asksAdmin } from "./view.ts";
import { busyOf } from "./words.ts";

export { failedWords, waitingWords } from "./words.ts";

/** A set-up press: `press` opens the confirm where an administrator is asked, else sends at once; `confirm` is the
 * dialog to place beside the button. */
export function useSetUpPress(
  state: ProtectionState,
  cause: Cause | null,
  send: () => void,
): { press: () => void; confirm: ReactNode } {
  const [open, setOpen] = useState(false);
  const press = () => {
    if (busyOf(state) !== null) return;
    if (asksAdmin(state.view, cause)) setOpen(true);
    else send();
  };
  return {
    press,
    confirm: (
      <ProtectionConfirm
        act="setup"
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
