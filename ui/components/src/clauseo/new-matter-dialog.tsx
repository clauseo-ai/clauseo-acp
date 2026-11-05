// New matter, from the rail. The rail's New matter opens the new matter form, the same part first
// launch opens on its first screen, in the system's dialog: a name as it stands on the cause list, a client if any,
// and the folder Clauseo makes for it under Documents. Focus starts on the name. Escape, Cancel or a press on the
// scrim closes it, and focus goes back to New matter.
import { Dialog as Base } from "@base-ui/react/dialog";
import { useState, type RefObject } from "react";
import { focusPlane } from "../ui/dialog-start.ts";
import { dialogPlace, dialogPlane, dialogTitle, scrim } from "../ui/recipes-overlays.ts";
import { cn } from "../lib/cn.ts";
import { useDisplay, type Platform } from "../lib/display.ts";
import { useMatterFormWords } from "../lib/words-matter-form.ts";
import { NewMatterForm, type NewMatterFormProps } from "./matter-form.tsx";

/** The folder Clauseo makes for a new matter, as the computer writes it: always in English. */
const newMatterFolder = (platform: Platform, name: string) =>
  ["Documents", "Clauseo", name].join(platform === "windows" ? "\\" : "/");

export function NewMatterDialog({
  open,
  onOpenChange,
  onCreate,
  name,
  client,
  details,
  folder,
  folderFor,
  pickFolder,
  error,
  nextError,
  openMatter,
  warning,
  blocked,
  testIds,
  finalFocus,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** Create the matter, pressed with a name, as the form gives it. Returning nothing closes the dialog at once. Given a
   * promise, Create stands pressed until it settles, and the dialog closes only if it resolves to anything but false;
   * a rejected promise, or false, keeps it open, for the caller's error line or a picker the lawyer cancelled. */
  onCreate: (name: string, client: string, next: string, nextDate: string | null) => unknown;
  /** Optional: what the fields hold as it opens. */
  name?: string;
  client?: string;
  /** Passed to the form: see NewMatterFormProps. Without folder or folderFor, the folder line shows the folder Clauseo
   * makes under Documents, as before. */
  details?: NewMatterFormProps["details"];
  folder?: NewMatterFormProps["folder"];
  folderFor?: NewMatterFormProps["folderFor"];
  pickFolder?: NewMatterFormProps["pickFolder"];
  error?: NewMatterFormProps["error"];
  nextError?: NewMatterFormProps["nextError"];
  openMatter?: NewMatterFormProps["openMatter"];
  warning?: NewMatterFormProps["warning"];
  blocked?: NewMatterFormProps["blocked"];
  testIds?: NewMatterFormProps["testIds"];
  /** Optional: where focus goes when it closes, New matter's button. Left out, the element focused before it opened. */
  finalFocus?: RefObject<HTMLElement | null>;
}) {
  const w = useMatterFormWords();
  const { platform } = useDisplay();
  const [{ mount, field }] = useState(focusPlane);
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
          finalFocus={finalFocus ?? true}
        >
          <Base.Title className={cn(dialogTitle, "mb-s4")}>{w("A new matter")}</Base.Title>
          <NewMatterForm
            folderFor={folderFor ?? ((n) => newMatterFolder(platform, n))}
            folder={folder}
            details={details}
            pickFolder={pickFolder}
            error={error}
            nextError={nextError}
            openMatter={openMatter}
            warning={warning}
            blocked={blocked}
            testIds={testIds}
            {...(name === undefined ? {} : { name })}
            {...(client === undefined ? {} : { client })}
            titled={false}
            inDialog
            onCreate={(n, c, next, date) => {
              const made = onCreate(n, c, next, date);
              if (!(made instanceof Promise)) {
                onOpenChange(false);
                return;
              }
              // closes only on success; the form keeps Create pressed meanwhile and swallows a rejection
              return made.then((ok) => {
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
