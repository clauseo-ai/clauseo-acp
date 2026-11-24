// New matter, from the rail: the parts' NewMatterDialog over matters.ts. It asks first: New folder or Existing folder.
// Existing folder opens the computer's own picker at once; cancelled, the question stays and nothing is sent; picked,
// the form holds that folder (existing_folder), the matter's name filled with the folder's own, and Create makes the
// matter where the folder is. New folder's form makes the folder under H20's default location (default_location), or
// under the folder Change… picks (new_folder). Create matter makes the matter from the answer's folder. The host's
// refusal is said in the form by its code (matter-form-check.ts), never its message, in the answer's form. Made, the new
// matter opens, the dialog closes and focus goes back to New matter (or, where the room folded the rail on onMade, to Matters). A folder inside
// or holding a matter's folder (matter.folder_inside_matter, matter.folder_holds_matter) names that matter when the list
// has it, and Open {name} opens it, as the rail does; with no name it takes the line of any folder refused.
// The first screen (UD1's first run) takes the form that offers both folders in place, titled, with Back (FirstMatter), and says a folder
// its Open the folder couldn't make a matter of by the same code's words (RefusedLine, with RefusedOpen's Open {name});
// all load with this chunk. A folder picked, either form asks the host about it first and warns or refuses as
// folder-check.tsx says.
import { useState, type RefObject } from "react";
import { NewMatterForm } from "@clauseo/components/clauseo/matter-form.tsx";
import { NewMatterDialog } from "@clauseo/components/clauseo/new-matter-dialog.tsx";
import { createErrorLine, nestedRefusal } from "@clauseo/components/clauseo/matter-form-check.ts";
import { isolate } from "@clauseo/components/lib/bidi.ts";
import { mt } from "@clauseo/components/lib/words-matters.ts";
import { Button } from "@clauseo/components/ui/button.tsx";
import { useWords } from "@clauseo/components/lib/words.ts";
import { toHostError } from "../transport/host-transport.ts";
import type { HostError } from "../transport/protocol.ts";
import { usePickedFolder } from "./folder-check.tsx";
import type { MatterInfo, NextMatterSource } from "./host-next.ts";
import type { Picked } from "./folder-check.tsx";
import { useAnswer, useDefaultLocation } from "./new-matter-answer.ts";
import type { Matters } from "./matters.ts";

/** Whether a refusal is of the folder itself: Create waits for another, and the line offers another (or, for a folder
 * nested with a matter on the list, Open {name} in its place). */
const refusesFolder = (code: string | undefined) =>
  code !== undefined && code.startsWith("matter.folder_");

/** The matter on the list a refusal names: a folder inside or holding its folder, where the host gave the matter. */
const namedBy = (matters: Matters, refused: Pick<HostError, "code" | "matter">) =>
  nestedRefusal(refused.code) && refused.matter !== undefined
    ? (matters.view().matters.find((m) => m.matter === refused.matter) ?? null)
    : null;

/** A refusal's line, and the matter on the list it names. */
const refusalOf = (
  matters: Matters,
  lang: Parameters<typeof createErrorLine>[0],
  refused: Pick<HostError, "code" | "matter">,
  path: string | undefined,
) => {
  const at = namedBy(matters, refused);
  return { line: createErrorLine(lang, refused.code, path, at?.name), at };
};

/** The form's test ids, as the e2e flows and the perf autoplay find them. */
const IDS = {
  name: "matter-name",
  existing: "matter-existing",
  made: "matter-made",
  create: "create-matter",
  open: "open-matter",
  choose: "folder-choose",
  anyway: "folder-anyway",
};

/** Where New folder makes the matter's folder: in the folder Change… picked, else the default location where the host
 * has one; null with neither. */
const madeIn = (
  parent: Picked | null,
  located: boolean,
): { source: NextMatterSource; path?: string } | null =>
  parent !== null
    ? { source: { new_folder: { folder: parent.folder } }, path: parent.display }
    : located
      ? { source: "default_location" }
      : null;

type NewMatterProps = {
  matters: Matters;
  /** Whether the host gives details (H5): the form asks for the client, and offers the default location. */
  detailed: boolean;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** Optional: the matter was made, before the dialog closes. */
  onMade?: () => void;
  /** Optional: Open {name}, on a folder nested with a matter on the list, opened that matter, before the dialog closes. */
  onOpened?: () => void;
  /** Where focus goes back as the dialog closes: New matter's button. */
  finalFocus: RefObject<HTMLElement | null>;
};

export function NewMatter(props: NewMatterProps) {
  const { matters, detailed, open, onOpenChange, onMade, onOpened, finalFocus } = props;
  const w = useWords();
  const pick = usePickedFolder(matters, w.lang);
  const [error, setError] = useState<{ line: string; code: string } | null>(null);
  // a new folder's parent, picked by Change…
  const [parent, setParent] = useState<Picked | null>(null);
  /** Choose folder…, a new folder picked dropping the last refusal's line. */
  const choose = async () => {
    const shown = await pick.pickExisting();
    if (shown !== null) {
      setError(null);
      setNamed(null);
    }
    return shown;
  };
  // the matter the refusal names, when the list has it: Open {name} opens it
  const [named, setNamed] = useState<MatterInfo | null>(null);
  const refusal = (refused: HostError, path: string | undefined) =>
    refusalOf(matters, w.lang, refused, path);
  // the existing folder's check refused it: its line at once
  const checked = pick.refused === null ? null : refusal(pick.refused, pick.existing?.display);
  const opens = named ?? checked?.at ?? null;
  // H20's default location, read as the form opens: the folder line before a pick
  const location = useDefaultLocation(matters, open && detailed);
  const forget = () => {
    pick.reset();
    setParent(null);
    setError(null);
    setNamed(null);
  };
  const { branch, last, picking, answer, reset } = useAnswer(choose, forget);
  const close = (next: boolean) => {
    if (!next) {
      forget();
      reset();
    }
    onOpenChange(next);
  };
  /** Change… in New folder: where the matter's folder is made instead; the create says any refusal. */
  const chooseParent = async () => {
    const picked = await pick.pickParent();
    if (picked === null) return null;
    setParent(picked);
    setError(null);
    setNamed(null);
    return picked.display;
  };
  /** The matter made from `source`, or the refusal said in the form, naming `path`. */
  const make = async (name: string, client: string, source: NextMatterSource, path?: string) => {
    const details = detailed && client.trim() !== "" ? { client: client.trim() } : {};
    try {
      await matters.create(name, source, details);
    } catch (thrown) {
      const refused = toHostError(thrown);
      const { line, at } = refusal(refused, path);
      setError({ line, code: refused.code });
      setNamed(at);
      return false;
    }
    forget();
    onMade?.();
    return true;
  };
  /** Open {name}: the matter a nested folder belongs with, the dialog closed. */
  const openNamed = (at: MatterInfo) => {
    matters.open(at.matter);
    onOpened?.();
    close(false);
  };
  const create = async (name: string, client: string) => {
    setError(null);
    setNamed(null);
    if (branch === "new") {
      const made = madeIn(parent, detailed && location !== null);
      return made !== null && make(name, client, made.source, made.path);
    }
    const where = await pick.where(false);
    if (where === null) return false;
    const { source, path } = where;
    if (where.refused !== undefined) {
      setError({ line: refusal(where.refused, path).line, code: where.refused.code });
      return false;
    }
    return make(name, client, source, path);
  };
  return (
    <NewMatterDialog
      open={open}
      onOpenChange={close}
      details={detailed}
      location={detailed ? location : null}
      {...(branch === null ? {} : { branch })}
      onBranch={(next) => void answer(next)}
      last={last}
      picking={picking}
      {...(branch === "existing" && pick.existing !== null
        ? { folder: pick.existing.display }
        : {})}
      pickFolder={branch === "new" ? chooseParent : choose}
      onMade={forget}
      error={error?.line ?? checked?.line ?? null}
      folderError={refusesFolder(error?.code ?? pick.refused?.code)}
      warning={pick.warning}
      scan={pick.scan}
      unpick={pick.unpick}
      opening={pick.opening}
      // a folder refused holds Create back until another is chosen; any other failure leaves it to try again. A new
      // folder's refusal may turn on its name, so Create stays free there
      blocked={branch !== "new" && (checked !== null || refusesFolder(error?.code))}
      {...(opens === null
        ? {}
        : { openMatter: { name: opens.name, open: () => openNamed(opens) } })}
      onCreate={create}
      // on the question, its two answers; in a form, the folder row's Choose folder… or Change…
      testIds={{ ...IDS, existing: branch === null ? "matter-existing" : "folder-change" }}
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
  const location = useDefaultLocation(matters, detailed);
  const [error, setError] = useState<{ line: string; code: string } | null>(null);
  // the matter the refusal names, when the list has it: Open {name} opens it
  const [named, setNamed] = useState<MatterInfo | null>(null);
  const choose = async () => {
    const shown = await pick.pickExisting();
    if (shown !== null) {
      setError(null);
      setNamed(null);
    }
    return shown;
  };
  const checked =
    pick.refused === null ? null : refusalOf(matters, w.lang, pick.refused, pick.existing?.display);
  const opens = named ?? checked?.at ?? null;
  const create = async (name: string, client: string, made: boolean) => {
    setError(null);
    setNamed(null);
    const where = await pick.where(made && detailed && location !== null);
    if (where === null) return false;
    const { source, path } = where;
    if (where.refused !== undefined) {
      setError({
        line: refusalOf(matters, w.lang, where.refused, path).line,
        code: where.refused.code,
      });
      return false;
    }
    onAct();
    const details = detailed && client.trim() !== "" ? { client: client.trim() } : {};
    try {
      await matters.create(name, source, details);
    } catch (thrown) {
      const refused = toHostError(thrown);
      const { line, at } = refusalOf(matters, w.lang, refused, path);
      setError({ line, code: refused.code });
      setNamed(at);
      return false;
    }
    return true;
  };
  return (
    <NewMatterForm
      titled
      details={detailed}
      location={detailed ? location : null}
      pickFolder={choose}
      onMade={() => {
        pick.reset();
        setError(null);
        setNamed(null);
      }}
      folderError={refusesFolder(error?.code ?? pick.refused?.code)}
      error={error?.line ?? checked?.line ?? null}
      warning={pick.warning}
      scan={pick.scan}
      unpick={pick.unpick}
      opening={pick.opening}
      blocked={pick.refused !== null || refusesFolder(error?.code)}
      {...(opens === null
        ? {}
        : { openMatter: { name: opens.name, open: () => matters.open(opens.matter) } })}
      onCreate={create}
      onCancel={onBack}
      testIds={IDS}
    />
  );
}

/** What the first screen's Open the folder heard back: the refusal's code and matter, and the folder as it reads. */
export type Refused = Pick<HostError, "code" | "matter"> & { path: string };

/** A folder the first screen's Open the folder couldn't make a matter of, said by the refusal's code, naming the matter
 * on the list a nested folder belongs with. */
export function RefusedLine({ matters, refused }: { matters: Matters; refused: Refused }) {
  const w = useWords();
  return <>{refusalOf(matters, w.lang, refused, refused.path).line}</>;
}

/** Under that line, for a folder inside or holding a matter on the list: Open {name}, which opens it. */
export function RefusedOpen({ matters, refused }: { matters: Matters; refused: Refused }) {
  const w = useWords();
  const at = namedBy(matters, refused);
  if (at === null) return null;
  return (
    <Button
      kind="tone"
      size="small"
      data-testid="open-matter"
      onClick={() => matters.open(at.matter)}
    >
      <span className="deva-nudge">{mt(w.lang, "Open {name}", { name: isolate(at.name) })}</span>
    </Button>
  );
}
