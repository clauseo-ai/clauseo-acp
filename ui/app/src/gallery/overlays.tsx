// The controls that raise a plane: each stands as its trigger, and opens on a click, a hover or focus.
import { IconHistory } from "@clauseo/components/icons/icons.tsx";
import { currentOs, revealKey, useUiWords } from "@clauseo/components/lib/words-ui.ts";
import {
  AlertDialog,
  AlertDialogClose,
  AlertDialogPopup,
  AlertDialogTrigger,
} from "@clauseo/components/ui/alert-dialog.tsx";
import { Button } from "@clauseo/components/ui/button.tsx";
import { Dialog, DialogClose, DialogPopup, DialogTrigger } from "@clauseo/components/ui/dialog.tsx";
import {
  Menu,
  MenuCheckboxItem,
  MenuGroup,
  MenuGroupLabel,
  MenuItem,
  MenuPopup,
  MenuSeparator,
  MenuTrigger,
} from "@clauseo/components/ui/menu.tsx";
import { Popover, PopoverCard, PopoverTrigger } from "@clauseo/components/ui/popover.tsx";
import {
  PreviewCard,
  PreviewCardPopup,
  PreviewCardTrigger,
} from "@clauseo/components/ui/preview-card.tsx";
import { Select } from "@clauseo/components/ui/select.tsx";
import { useToasts } from "@clauseo/components/ui/toast-queue.ts";
import { Tooltip } from "@clauseo/components/ui/tooltip.tsx";
import { useState } from "react";
import { Section, State } from "./layout.tsx";
import type { LiteChoice } from "./options.ts";

export function Overlays() {
  const w = useUiWords();
  const toasts = useToasts();
  const [pinned, setPinned] = useState(true);
  const [lite, setLite] = useState<LiteChoice>("auto");
  const choices = [
    { value: "auto", label: w.ui("Automatic") },
    { value: "on", label: w.ui("On") },
    { value: "off", label: w.ui("Off") },
  ] as const;
  return (
    <Section title="Overlays">
      <State name="Tooltip">
        <Tooltip content="History">
          <Button kind="quiet" shape="icon" aria-label="History">
            <IconHistory size={16} />
          </Button>
        </Tooltip>
      </State>
      <State name="Popover">
        <Popover>
          <PopoverTrigger render={<Button kind="tone" />}>{w.ui("Folder")}</PopoverTrigger>
          <PopoverCard aria-label={w.ui("Folder")} align="start" className="p-s4 text-small">
            {w.say(w.ui("Clauseo reads only the papers in this folder."))}
          </PopoverCard>
        </Popover>
      </State>
      <State name="Preview card">
        <PreviewCard>
          <PreviewCardTrigger href="#lease">Lease.docx</PreviewCardTrigger>
          <PreviewCardPopup className="p-s4 text-small">Notice to the bank.docx</PreviewCardPopup>
        </PreviewCard>
      </State>
      <State name="Menu">
        <Menu>
          <MenuTrigger render={<Button kind="tone" />}>
            {w.ui("Actions for {name}", { name: "Lease" })}
          </MenuTrigger>
          <MenuPopup>
            <MenuGroup>
              <MenuGroupLabel lang="en">Lease</MenuGroupLabel>
              <MenuItem label={w.ui("Rename")} />
              <MenuItem label={w.ui(revealKey(currentOs()))} />
              <MenuCheckboxItem
                label={w.ui("Pin to the top")}
                checked={pinned}
                onCheckedChange={setPinned}
              />
            </MenuGroup>
            <MenuSeparator />
            <MenuItem
              label={w.ui("Remove from the list")}
              note={w.ui("The folder and its papers stay where they are.")}
            />
            <MenuItem label={w.ui("Undo anyway")} disabled />
          </MenuPopup>
        </Menu>
      </State>
      <State name="Select, disabled">
        <span className="flex items-center gap-s2">
          <Select
            label={w.ui("Lite mode")}
            value={lite}
            options={choices}
            onValueChange={setLite}
          />
          <Select
            label={w.ui("Lite mode")}
            value="auto"
            options={choices}
            onValueChange={() => {}}
            disabled
          />
        </span>
      </State>
      <State name="Dialog">
        <Dialog>
          <DialogTrigger render={<Button kind="tone" />}>{w.ui("Restore")}</DialogTrigger>
          <DialogPopup
            title={w.ui("Restore the {time} version of the petition?", {
              time: w.clock("3:58 pm"),
            })}
            body={w.ui("The current version stays in History.")}
            actions={
              <>
                <DialogClose render={<Button kind="quiet" />}>{w.ui("Cancel")}</DialogClose>
                <DialogClose render={<Button kind="primary" />}>{w.ui("Restore")}</DialogClose>
              </>
            }
          />
        </Dialog>
      </State>
      <State name="Alert dialog">
        <AlertDialog>
          <AlertDialogTrigger render={<Button kind="tone" />}>
            {w.ui("Undo anyway")}
          </AlertDialogTrigger>
          <AlertDialogPopup
            undoable
            title={w.say(w.ui("Lease.docx changed after Clauseo's edit."))}
            body={w.say(
              w.ui(
                "Undo anyway puts back the version from before Clauseo's edit. The changes made since stay in History.",
              ),
            )}
            actions={
              <>
                <AlertDialogClose render={<Button kind="quiet" />}>
                  {w.ui("Keep current")}
                </AlertDialogClose>
                <AlertDialogClose render={<Button kind="primary" />}>
                  {w.ui("Undo anyway")}
                </AlertDialogClose>
              </>
            }
          />
        </AlertDialog>
      </State>
      <State name="Toast">
        <Button kind="tone" onClick={() => toasts.show(w.ui("List of citations copied."))}>
          {w.ui("Copy the list of citations")}
        </Button>
      </State>
    </Section>
  );
}
