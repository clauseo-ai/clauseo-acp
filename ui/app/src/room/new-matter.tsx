// New matter, from the rail: the parts' NewMatterDialog over matters.ts. Two sources. Change picks an existing
// folder (existing_folder) and shows how it reads; cancelled, nothing changes. Create the matter makes it from that
// folder; with no folder picked, in H20's default location where the host has one (no picker), else in a parent the
// picker asks for (new_folder), and a cancelled picker sends nothing and leaves the form open. The host's refusal is
// said in the form by its code (matter-form-check.ts), never its message. Made, the new matter opens, the dialog
// closes and focus goes back to New matter (or, where the room folded the rail on onMade, to Matters). A folder that is
// already a matter (matter.folder_is_matter) names that matter when the list has it, and Open it opens it, as the rail does.
import { useEffect, useState, type RefObject } from "react";
import { NewMatterDialog } from "@clauseo/components/clauseo/new-matter-dialog.tsx";
import { createErrorLine } from "@clauseo/components/clauseo/matter-form-check.ts";
import { useWords } from "@clauseo/components/lib/words.ts";
import { toHostError } from "../transport/host-transport.ts";
import type { FolderRefId, MatterSource } from "../transport/protocol.ts";
import type { MatterInfo, NextMatterSource } from "./host-next.ts";
import type { Matters } from "./matters.ts";

type Picked = { folder: FolderRefId; display: string };

/** `<location>/<name>`, with the separator the location is written in. */
const under = (location: string, name: string) => {
  const sep = location.includes("\\") && !location.includes("/") ? "\\" : "/";
  return `${location.replace(/[\\/]+$/, "")}${sep}${name}`;
};

export function NewMatter({
  matters,
  detailed,
  open,
  onOpenChange,
  onMade,
  onOpened,
  finalFocus,
}: {
  matters: Matters;
  /** Whether the host gives details (H5): the form asks for the client and the next date. */
  detailed: boolean;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** Optional: the matter was made, before the dialog closes. */
  onMade?: () => void;
  /** Optional: Open it, on a folder that is already a matter, opened that matter, before the dialog closes. */
  onOpened?: () => void;
  /** Where focus goes back as the dialog closes: New matter's button. */
  finalFocus: RefObject<HTMLElement | null>;
}) {
  const w = useWords();
  const [existing, setExisting] = useState<Picked | null>(null);
  const [location, setLocation] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  // the matter the refusal names, when the list has it: Open it opens it
  const [named, setNamed] = useState<MatterInfo | null>(null);
  // H20's default location, read as the form opens: the folder line before a pick
  useEffect(() => {
    if (!open || !detailed) return;
    let live = true;
    void matters.defaultLocation().then((display) => {
      if (live) setLocation(display);
    });
    return () => {
      live = false;
    };
  }, [open, detailed, matters]);
  const close = (next: boolean) => {
    if (!next) {
      setExisting(null);
      setError(null);
      setNamed(null);
    }
    onOpenChange(next);
  };
  const create = async (name: string, client: string, nextDate: string | null) => {
    setError(null);
    setNamed(null);
    let source: NextMatterSource;
    let path = existing?.display;
    if (existing !== null) source = { existing_folder: { folder: existing.folder } };
    else if (detailed && location !== null) source = "default_location";
    else {
      const parent = await matters.pickFolder();
      if (parent === null) return false;
      path = parent.display;
      source = { new_folder: { folder: parent.folder } } satisfies MatterSource;
    }
    const details = detailed
      ? {
          ...(client.trim() === "" ? {} : { client: client.trim() }),
          ...(nextDate === null ? {} : { next_date: nextDate }),
        }
      : {};
    try {
      await matters.create(name, source, details);
    } catch (thrown) {
      const refusal = toHostError(thrown);
      const at =
        refusal.code === "matter.folder_is_matter" && refusal.matter !== undefined
          ? (matters.view().matters.find((m) => m.matter === refusal.matter) ?? null)
          : null;
      setError(createErrorLine(w.lang, refusal.code, path, at?.name));
      setNamed(at);
      return false;
    }
    setExisting(null);
    onMade?.();
    return true;
  };
  return (
    <NewMatterDialog
      open={open}
      onOpenChange={close}
      details={detailed}
      {...(existing !== null
        ? { folder: existing.display }
        : location !== null && detailed
          ? { folderFor: (name: string) => under(location, name) }
          : { folder: null })}
      pickFolder={async () => {
        const picked = await matters.pickFolder();
        if (picked === null) return null;
        setExisting(picked);
        return picked.display;
      }}
      error={error}
      {...(named === null
        ? {}
        : {
            openMatter: () => {
              matters.open(named.matter);
              onOpened?.();
              close(false);
            },
          })}
      onCreate={(name, client, _next, nextDate) => create(name, client, nextDate)}
      testIds={{
        name: "matter-name",
        existing: "matter-existing",
        create: "create-matter",
        open: "open-matter",
      }}
      finalFocus={finalFocus}
    />
  );
}
