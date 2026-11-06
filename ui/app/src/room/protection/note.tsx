// UC: the Not protected note in the room, the chunk note-load.ts loads: the cause's body (components' protection.tsx),
// its retry through the set-up confirm where Windows' administrator prompt follows (setup.tsx), the waiting line in
// the button's place and a refusal's words under the body. When the matter becomes protected the note closes. The
// firm's notice (required.tsx) rides in the same chunk.
import { NotProtectedNote, type NoteWhy } from "@clauseo/components/clauseo/protection.tsx";
import { useDisplay } from "@clauseo/components/lib/display.ts";
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

/** The note's `why` for a cause. */
const noteWhy = (cause: Cause): NoteWhy =>
  cause.group === "setup"
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
  const { lang } = useDisplay();
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
  const retry = canRetry(cause);
  return (
    <>
      <NotProtectedNote
        open={open || auto}
        matter={matter}
        why={noteWhy(cause)}
        {...(cause.group === "pool" ? { pool: poolOf(cause), addRoom: retry } : {})}
        named1385={named1385(cause)}
        waiting={retry ? waitingWords(lang, state, cause) : null}
        failed={retry ? failedWords(lang, state) : null}
        onClose={close}
        onProtect={retry ? press : undefined}
      />
      {confirm}
    </>
  );
}
