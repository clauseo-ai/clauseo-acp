// The open matter's standing line where Windows' Controlled folder access protects its folder (issue 5, after the
// first-start step): the block falls on Claude's and ChatGPT's tools (PowerShell), no file is made, and the lawyer
// hears only "file not found" from the assistant, with nothing saying Windows did it. The line says so, in stage E's
// words, and the way that works today: move the folder out of Documents in File Explorer (Show in File Explorer), then
// choose it here (Locate folder's flow, matters.locate, relocate_matter). Where the host can set where new matters go
// (permissions-host.ts's chooseMattersFolder, set_default_location, still to come), its choice joins; hidden until
// then. It moves nothing: this matter stays until the lawyer moves its folder. The rows are read
// once per window (the desktop's permissions(), silent), so opening another matter asks nothing again.
import { useEffect, useState } from "react";
import { WINDOWS } from "@clauseo/components/clauseo/first-launch/windows-words.ts";
import { FolderChoiceButton } from "@clauseo/components/clauseo/first-launch/folder-choice.tsx";
import { Say } from "@clauseo/components/clauseo/say.tsx";
import { IconNeedsPermission } from "@clauseo/components/icons/icons.tsx";
import { fl } from "@clauseo/components/lib/words-first-launch.ts";
import { mt } from "@clauseo/components/lib/words-matters.ts";
import { ui } from "@clauseo/components/lib/words-ui.ts";
import { useWords } from "@clauseo/components/lib/words.ts";
import { Button } from "@clauseo/components/ui/button.tsx";
import type { HostTransport } from "../transport/host-transport.ts";
import type { MatterId } from "../transport/protocol.ts";
import {
  blockingRow,
  permissionsHostOf,
  type PermissionRow,
  type PermissionsHost,
} from "./first-run/permissions-host.ts";

/** Each window's rows, read once. */
const read = new WeakMap<
  HostTransport,
  { host: PermissionsHost; rows: Promise<PermissionRow[]> }
>();

function rowsOf(transport: HostTransport) {
  let known = read.get(transport);
  if (known === undefined) {
    const host = permissionsHostOf(transport);
    known = { host, rows: host.list() };
    read.set(transport, known);
  }
  return known;
}

/** The line, or nothing where the matter's folder isn't protected. */
export function MatterBlockedLine({
  transport,
  matter,
  folder,
  onReveal,
  onLocate,
}: {
  transport: HostTransport;
  matter: MatterId;
  /** The matter's folder as it reads (MatterInfo's folder_display). */
  folder: string | undefined;
  /** Show in File Explorer, for this matter. */
  onReveal: () => void;
  /** Locate folder's flow, for this matter: the picker, then relocate_matter. */
  onLocate: () => void;
}) {
  const { lang } = useWords();
  const [{ host, rows }] = useState(() => rowsOf(transport));
  const [listed, setListed] = useState<PermissionRow[]>([]);
  useEffect(() => {
    let live = true;
    void rows.then((r) => {
      if (live) setListed(r);
    });
    return () => {
      live = false;
    };
  }, [rows]);
  if (blockingRow(listed, folder) === null) return null;
  const choose = host.chooseMattersFolder;
  return (
    <div
      className="mx-auto box-content flex max-w-measure items-start gap-s1-5 px-thread-pad pt-s6 pb-s2 text-small max-mid:mx-0 max-mid:max-w-none max-mid:px-s4 max-mid:pt-0 max-mid:pb-s4"
      role="status"
      data-testid="matter-blocked"
      data-matter={matter}
    >
      <IconNeedsPermission size={16} className="flex-none mt-s0-5 text-primary" />
      <div className="grid min-w-0 gap-s1">
        <p className="font-chip text-primary">
          <Say>{mt(lang, "Windows' Controlled folder access blocks saving in this folder")}</Say>
        </p>
        <p className="text-quiet">
          <Say>
            {[
              fl(lang, WINDOWS.why),
              mt(
                lang,
                "Move this matter's folder out of Documents in File Explorer, then choose it here.",
              ),
            ].join(" ")}
          </Say>
        </p>
        {/* the quiet buttons' words line up with the words above */}
        <div className="-ml-s3 flex flex-wrap gap-s1">
          <Button kind="quiet" size="small" onClick={onReveal} data-testid="matter-blocked-reveal">
            {ui(lang, "Show in File Explorer")}
          </Button>
          <Button kind="quiet" size="small" onClick={onLocate} data-testid="matter-blocked-locate">
            {mt(lang, "Choose the moved folder")}
          </Button>
          {choose !== undefined && <FolderChoiceButton onChoose={choose} />}
        </div>
      </div>
    </div>
  );
}
