// New matter, from the rail: the parts' NewMatterDialog over matters.ts. Two sources. Change picks an existing
// folder (existing_folder) and shows how it reads; cancelled, nothing changes. Create the matter makes it from that
// folder; with no folder picked, in H20's default location where the host has one (no picker), else in a parent the
// picker asks for (new_folder), and a cancelled picker sends nothing and leaves the form open. The host's refusal is
// said in the form by its code (matter-form-check.ts), never its message. Made, the new matter opens, the dialog
// closes and focus goes back to New matter (or, where the room folded the rail on onMade, to Matters). A folder that is
// already a matter (matter.folder_is_matter) names that matter when the list has it, and Open it opens it, as the rail does.
// The first screen (UD1's first run) takes the same form in place, titled, with Back (FirstMatter), and says a folder
// its Open the folder couldn't make a matter of by the same code's words (RefusedLine); both load with this chunk. A
// folder picked, either form asks the host about it first and warns or refuses as folder-check.tsx says.
import { useEffect, useState, type RefObject } from "react";
import { NewMatterForm } from "@clauseo/components/clauseo/matter-form.tsx";
import { NewMatterDialog } from "@clauseo/components/clauseo/new-matter-dialog.tsx";
import { createErrorLine } from "@clauseo/components/clauseo/matter-form-check.ts";
import { useWords } from "@clauseo/components/lib/words.ts";
import { toHostError } from "../transport/host-transport.ts";
import type { HostError } from "../transport/protocol.ts";
import { usePickedFolder, type Picked } from "./folder-check.tsx";
import type { MatterInfo } from "./host-next.ts";
import type { Matters } from "./matters.ts";

/** `<location>/<name>`, with the separator the location is written in. */
const under = (location: string, name: string) => {
  const sep = location.includes("\\") && !location.includes("/") ? "\\" : "/";
  return `${location.replace(/[\\/]+$/, "")}${sep}${name}`;
};

/** The form's folder line: the existing folder picked; else where the matter goes, under the default location or a
 * parent Create picked; else none. */
const folderLine = (existing: Picked | null, parent: Picked | null, location: string | null) =>
  existing !== null
    ? { folder: existing.display }
    : location !== null
      ? { folderFor: (name: string) => under(location, name) }
      : parent !== null
        ? { folderFor: (name: string) => under(parent.display, name) }
        : { folder: null };

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
  const pick = usePickedFolder(matters, w.lang);
  const [location, setLocation] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  // the matter the refusal names, when the list has it: Open it opens it
  const [named, setNamed] = useState<MatterInfo | null>(null);
  /** A refusal's line, and the matter on the list it names. */
  const refusal = (refused: HostError, path: string | undefined) => {
    const at =
      refused.code === "matter.folder_is_matter" && refused.matter !== undefined
        ? (matters.view().matters.find((m) => m.matter === refused.matter) ?? null)
        : null;
    return { line: createErrorLine(w.lang, refused.code, path, at?.name), at };
  };
  // the existing folder's check refused it: its line at once
  const checked = pick.refused === null ? null : refusal(pick.refused, pick.existing?.display);
  const opens = named ?? checked?.at ?? null;
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
      pick.reset();
      setError(null);
      setNamed(null);
    }
    onOpenChange(next);
  };
  const create = async (name: string, client: string, nextDate: string | null) => {
    setError(null);
    setNamed(null);
    const where = await pick.where(detailed && location !== null);
    if (where === null) return false;
    const { source, path } = where;
    if (where.refused !== undefined) {
      setError(refusal(where.refused, path).line);
      return false;
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
      const { line, at } = refusal(toHostError(thrown), path);
      setError(line);
      setNamed(at);
      return false;
    }
    pick.reset();
    onMade?.();
    return true;
  };
  return (
    <NewMatterDialog
      open={open}
      onOpenChange={close}
      details={detailed}
      {...folderLine(pick.existing, pick.parent, detailed ? location : null)}
      pickFolder={pick.pickExisting}
      error={error ?? checked?.line ?? null}
      warning={pick.warning}
      scan={pick.scan}
      unpick={pick.unpick}
      blocked={checked !== null}
      {...(opens === null
        ? {}
        : {
            openMatter: () => {
              matters.open(opens.matter);
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
        choose: "folder-choose",
        anyway: "folder-anyway",
      }}
      finalFocus={finalFocus}
    />
  );
}

/** The first screen's new matter (UD1): the form in place of the question, titled, Back going back to it. Its folder as
 * New matter's dialog makes it; `onAct` hears the first act just before the matter is made. */
export function FirstMatter({
  matters,
  onAct,
  onBack,
}: {
  matters: Matters;
  onAct: () => void;
  onBack: () => void;
}) {
  const w = useWords();
  const detailed = matters.view().detailed;
  const pick = usePickedFolder(matters, w.lang);
  const [location, setLocation] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    if (!detailed) return;
    let live = true;
    void matters.defaultLocation().then((display) => {
      if (live) setLocation(display);
    });
    return () => {
      live = false;
    };
  }, [detailed, matters]);
  const create = async (name: string, client: string, nextDate: string | null) => {
    setError(null);
    const where = await pick.where(detailed && location !== null);
    if (where === null) return false;
    const { source, path } = where;
    if (where.refused !== undefined) {
      setError(createErrorLine(w.lang, where.refused.code, path));
      return false;
    }
    onAct();
    const details = detailed
      ? {
          ...(client.trim() === "" ? {} : { client: client.trim() }),
          ...(nextDate === null ? {} : { next_date: nextDate }),
        }
      : {};
    try {
      await matters.create(name, source, details);
    } catch (thrown) {
      setError(createErrorLine(w.lang, toHostError(thrown).code, path));
      return false;
    }
    return true;
  };
  return (
    <NewMatterForm
      titled
      details={detailed}
      {...folderLine(pick.existing, pick.parent, detailed ? location : null)}
      pickFolder={pick.pickExisting}
      error={
        error ??
        (pick.refused === null
          ? null
          : createErrorLine(w.lang, pick.refused.code, pick.existing?.display))
      }
      warning={pick.warning}
      scan={pick.scan}
      unpick={pick.unpick}
      blocked={pick.refused !== null}
      onCreate={(name, client, _next, nextDate) => create(name, client, nextDate)}
      onCancel={onBack}
      testIds={{
        name: "matter-name",
        existing: "matter-existing",
        create: "create-matter",
        choose: "folder-choose",
        anyway: "folder-anyway",
      }}
    />
  );
}

/** A folder the first screen's Open the folder couldn't make a matter of, said by the refusal's code. */
export function RefusedLine({ code, path }: { code: string; path: string }) {
  const w = useWords();
  return <>{createErrorLine(w.lang, code, path)}</>;
}
