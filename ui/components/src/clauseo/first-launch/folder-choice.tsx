// "Choose a matters folder outside Documents" (issue 5), where the host can set where new matters go (stage E's
// set_default_location): the button, then its outcome in a plain line under it. New matters go in the folder chosen;
// existing matters stay, and nothing on disk is made or moved, so no line promises a move. The first screen's
// Windows row and the open matter's line (room/matter-blocked.tsx) both use it; the caller runs the picker and the
// command (room/first-run/permissions-host.ts's chooseMattersFolder) and hands back what came of it.
import { useState } from "react";
import { useDisplay } from "../../lib/display.ts";
import { mt } from "../../lib/words-matters.ts";
import { fl } from "../../lib/words-first-launch.ts";
import { Button } from "../../ui/button.tsx";
import { Say } from "../say.tsx";
import { WINDOWS } from "./windows-words.ts";

/** What a matters-folder choice came to: the folder new matters go in now, as it reads, or the host's refusal. */
export type FolderChoice =
  | { done: string }
  | { refused: "settings.locked" | "command.invalid" | "store.io" | "other" };

/** The line for an outcome. */
function said(lang: Parameters<typeof mt>[0], choice: FolderChoice): string {
  if ("done" in choice)
    return mt(
      lang,
      "New matters now go in {folder}. Matters you already have stay where they are.",
      {
        folder: choice.done,
      },
    );
  switch (choice.refused) {
    case "settings.locked":
      return mt(lang, "Your firm sets where new matters go, so it can't be changed here.");
    case "command.invalid":
      return mt(lang, "Clauseo couldn't use that folder. Choose it again.");
    default:
      return mt(lang, "Clauseo couldn't save the change. Try again.");
  }
}

/** The button and, once pressed and answered, its line. A second press while one runs does nothing. */
export function FolderChoiceButton({
  onChoose,
  kind = "quiet",
}: {
  /** The picker, then the command: its outcome, or null when the lawyer cancels. */
  onChoose: () => Promise<FolderChoice | null>;
  kind?: "tone" | "quiet";
}) {
  const { lang } = useDisplay();
  const [busy, setBusy] = useState(false);
  const [choice, setChoice] = useState<FolderChoice | null>(null);
  const press = () => {
    if (busy) return;
    setBusy(true);
    onChoose().then(
      (c) => {
        setBusy(false);
        if (c !== null) setChoice(c);
      },
      () => {
        setBusy(false);
        setChoice({ refused: "other" });
      },
    );
  };
  return (
    <>
      <Button
        kind={kind}
        {...(kind === "quiet" ? { size: "small" as const } : { className: "fl-perm-act" })}
        onClick={press}
        disabled={busy}
        aria-busy={busy ? true : undefined}
        data-testid="folder-choice"
      >
        {fl(lang, WINDOWS.act)}
      </Button>
      {choice !== null && (
        <p
          // quiet, it stands in a row of quiet buttons pulled out by their inset: its words keep the row's edge
          className={
            kind === "quiet"
              ? "basis-full ps-s3 text-small text-primary"
              : "basis-full text-small text-primary"
          }
          role="status"
          data-testid="folder-choice-said"
          data-outcome={"done" in choice ? "done" : choice.refused}
        >
          <Say>{said(lang, choice)}</Say>
        </p>
      )}
    </>
  );
}
