// New matter, from the rail. The rail's New matter opens the new matter form, the same part first
// launch opens on its first screen, in the system's dialog: a name as it stands on the cause list, a client if any,
// and the folder Clauseo makes for it under Documents. Focus starts on the name. Escape, Cancel or a press on the
// scrim closes it, and focus goes back to New matter.
import { Dialog as Base } from "@base-ui/react/dialog";
import { useState } from "react";
import { focusPlane } from "../ui/dialog-start.ts";
import { dialogPlace, dialogPlane, dialogTitle, scrim } from "../ui/recipes-overlays.ts";
import { cn } from "../lib/cn.ts";
import { useDisplay, type Platform } from "../lib/display.ts";
import { useMatterFormWords } from "../lib/words-matter-form.ts";
import { NewMatterForm } from "./matter-form.tsx";

/** The folder Clauseo makes for a new matter, as the computer writes it: always in English. */
const newMatterFolder = (platform: Platform, name: string) =>
  ["Documents", "Clauseo", name].join(platform === "windows" ? "\\" : "/");

export function NewMatterDialog({
  open,
  onOpenChange,
  onCreate,
  name,
  client,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** Create the matter, pressed with a name. */
  onCreate: (name: string, client: string) => void;
  /** Optional: what the fields hold as it opens. */
  name?: string;
  client?: string;
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
        >
          <Base.Title className={cn(dialogTitle, "mb-s4")}>{w("A new matter")}</Base.Title>
          <NewMatterForm
            folderFor={(n) => newMatterFolder(platform, n)}
            {...(name === undefined ? {} : { name })}
            {...(client === undefined ? {} : { client })}
            titled={false}
            inDialog
            onCreate={(n, c) => {
              onCreate(n, c);
              onOpenChange(false);
            }}
            onCancel={() => onOpenChange(false)}
          />
        </Base.Popup>
      </Base.Portal>
    </Base.Root>
  );
}
