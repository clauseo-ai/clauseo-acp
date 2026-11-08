// UC: the Not protected note in the room, the chunk note-load.ts loads: the cause's body
// (components' protection.tsx), its retry through the set-up confirm where Windows' administrator
// prompt follows (setup.tsx), the waiting line in the button's place and a refusal's words under
// the body. Design B: a block says its reason's line (blocked.ts), and where the repair is the
// installer (a missing admin copy, or R17 PR-E M4's broken set-up, which then says to ask IT if
// that doesn't help) the retry gives way to its line and, on a personal install, "Get the installer". R17
// PR-E: a full pool's body follows its why, and "Free up places" (retry_protection, behind
// Windows' prompt) shows only while places need a reset; an item a place didn't hand back
// (pool_owned) has no retry, and its IT line names where when the note is given a refused turn's
// `at`. When the matter becomes protected the note closes. The firm's notice (required.tsx) rides in the same
// chunk.
import { NotProtectedNote } from "@clauseo/components/clauseo/protection.tsx";
import { useDisplay } from "@clauseo/components/lib/display.ts";
import { blockLine, firmInstall, repairByInstaller, repairLine } from "./blocked.ts";
import { pw } from "@clauseo/components/lib/words-protection.ts";
import { openInstallerPage, repairPage } from "./installer.ts";
import { useEffect, useState } from "react";
import type { MatterId } from "../../transport/protocol.ts";
import { explainDue, markExplained } from "./explained.ts";
import type { ProtectionStore } from "../../store/protection.ts";
import { useProtection } from "../../store/protection-run.ts";
import type { HostTransport } from "../../transport/host-transport.ts";
import { retryProtection } from "./commands.ts";
import { failedWords, useSetUpPress, waitingWords } from "./setup.tsx";
import { canFreeUp, canRetry } from "./view.ts";
import { poolOf, whyOf } from "./detail.ts";
import { noteWhy } from "./note-why.ts";
import type { Cause } from "./causes.ts";

export function ProtectionNote({
  transport,
  store,
  matterId,
  matter,
  cause,
  open,
  where = null,
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
  /** Optional, for pool_owned: the item a refused turn named (its `at`), for the IT line. */
  where?: string | null;
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
  const close = () => {
    setAuto(false);
    onClose();
  };
  const gone = cause === null;
  useEffect(() => {
    if (gone && open) onClose();
  }, [gone, open, onClose]);
  if (cause === null) return null;
  const repair = repairByInstaller(cause);
  const retry = !repair && canRetry(cause);
  const freeUp = canFreeUp(cause);
  const block = blockLine(lang, cause);
  const page = repair && !firmInstall(state.view, state.managedBy) ? repairPage(display) : null;
  const extra = [
    ...(block === null ? [] : [block]),
    ...(repair ? [repairLine(lang, state.view, state.managedBy)] : []),
    // M4: a broken set-up's last resort, after the repair
    ...(cause.name === "set_up_failed"
      ? [pw(lang, "If that doesn't help, ask your IT team.")]
      : []),
  ];
  return (
    <>
      <NotProtectedNote
        open={open || auto}
        matter={matter}
        why={noteWhy(cause)}
        {...(cause.group === "pool" ? { pool: poolOf(cause), addRoom: retry } : {})}
        waiting={retry || freeUp ? waitingWords(lang, state, cause) : null}
        failed={retry || freeUp ? failedWords(lang, state) : null}
        extra={extra}
        where={where}
        onFreeUp={freeUp ? press : undefined}
        onGetInstaller={page === null ? undefined : () => void openInstallerPage(transport, page)}
        onClose={close}
        onProtect={retry ? press : undefined}
      />
      {confirm}
    </>
  );
}
