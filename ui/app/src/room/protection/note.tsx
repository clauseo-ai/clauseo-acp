// UC: the Not protected note in the room, the chunk note-load.ts loads: the cause's body
// (components' protection.tsx), its retry through the set-up confirm where Windows' administrator
// prompt follows (setup.tsx), the waiting line in the button's place and a refusal's words under
// the body. Design B: a block says its reason's line (blocked.ts), and where the repair is the
// installer the retry gives way to its line and, on a personal install, "Get the installer", with
// "Download and run the installer again" before it where the app can fetch the installer itself
// (repair.ts). After that installer the note opens by itself once per run: on the copy still
// missing, or with focus on its Set up once the copy is there. When the matter becomes protected
// the note closes. The firm's notice (required.tsx) rides in the same
// chunk.
import { NotProtectedNote, type NoteWhy } from "@clauseo/components/clauseo/protection.tsx";
import { useDisplay } from "@clauseo/components/lib/display.ts";
import { blockLine, blockOf, firmInstall, needsInstaller, repairLine } from "./blocked.ts";
import { openInstallerPage, repairPage } from "./installer.ts";
import { landingDue, markLanded, repairView } from "./repair.ts";
import { useEffect, useState } from "react";
import type { MatterId } from "../../transport/protocol.ts";
import { explainDue, markExplained } from "./explained.ts";
import type { ProtectionStore } from "../../store/protection.ts";
import { useProtection } from "../../store/protection-run.ts";
import type { HostTransport } from "../../transport/host-transport.ts";
import type { Cause } from "./causes.ts";
import { retryProtection } from "./commands.ts";
import { failedWords, useSetUpPress, waitingWords } from "./setup.tsx";
import { canRetry } from "./view.ts";
import { named1385, poolOf, whyOf } from "./detail.ts";

/** The note's `why` for a cause: a missing admin copy reads as not set up yet. */
const noteWhy = (cause: Cause): NoteWhy =>
  blockOf(cause) === "admin_copy_missing"
    ? "needs_set_up"
    : blockOf(cause) !== null
      ? "blocked"
      : cause.group === "setup"
        ? (cause.name as NoteWhy)
        : cause.group === "off"
          ? "off"
          : cause.group === "pool"
            ? "pool"
            : whyOf(cause) === "not_built"
              ? "not_built"
              : "cant";

export function ProtectionNote({
  transport,
  store,
  matterId,
  matter,
  cause,
  open,
  onClose,
}: {
  /** The open matter, for "explained once": the note mounts once per matter and cause group (its key). */
  matterId: MatterId;
  transport: HostTransport;
  store: ProtectionStore;
  /** The matter's name as filed. */
  matter: string;
  /** The open matter's cause; null once it is protected (or not known), which closes the note. */
  cause: Cause | null;
  open: boolean;
  onClose: () => void;
}) {
  const display = useDisplay();
  const { lang } = display;
  const state = useProtection(store);
  const { press, confirm } = useSetUpPress(
    state,
    cause,
    () => void retryProtection(transport, store),
  );
  // the first time a room opens on a matter in this state it opens by itself (explained.ts), and is marked explained
  // at once, so the next room doesn't open it again
  // a build without protection (every real build until desktop wires it in) reads calmly: the mark and the line say
  // it, and the note waits for a press
  const [auto, setAuto] = useState(
    () => cause !== null && whyOf(cause) !== "not_built" && explainDue(cause.group, matterId),
  );
  const group = cause?.group ?? null;
  useEffect(() => {
    if (auto && group !== null) markExplained(group, matterId);
  }, [auto, group, matterId]);
  // after a repair's installer, once per run: the card the lawyer comes back to (repair.ts)
  const land = landingDue(state, cause);
  // the note opens on it, once: kept here from the render that saw it due, and marked in the store
  const [landing, setLanding] = useState(false);
  if (land && !landing) setLanding(true);
  useEffect(() => {
    if (landing) markLanded(store);
  }, [landing, store]);
  const close = () => {
    setAuto(false);
    setLanding(false);
    onClose();
  };
  const gone = cause === null;
  useEffect(() => {
    if (gone && open) onClose();
  }, [gone, open, onClose]);
  if (cause === null) return null;
  const repair = needsInstaller(cause);
  const retry = !repair && canRetry(cause);
  const block = blockLine(lang, cause);
  const page = repair && !firmInstall(state.view, state.managedBy) ? repairPage(display) : null;
  const run = repairView(lang, transport, store, state, cause);
  const extra = [
    ...(block === null ? [] : [block]),
    ...(repair ? [repairLine(lang, state.view, state.managedBy)] : []),
    ...(run.returned === null ? [] : [run.returned]),
  ];
  return (
    <>
      <NotProtectedNote
        open={open || auto || landing}
        matter={matter}
        why={noteWhy(cause)}
        {...(cause.group === "pool" ? { pool: poolOf(cause), addRoom: retry } : {})}
        named1385={named1385(cause)}
        waiting={retry ? waitingWords(lang, state, cause) : null}
        failed={retry ? failedWords(lang, state) : run.failed}
        extra={extra}
        onGetInstaller={page === null ? undefined : () => void openInstallerPage(transport, page)}
        onRepair={run.onRepair}
        onCancel={run.onCancel}
        repairWait={run.wait}
        onClose={close}
        onProtect={retry ? press : undefined}
      />
      {confirm}
    </>
  );
}
