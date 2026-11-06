// UC: the room's protection, for the open matter: the composer's line, the toolbar's mark, the firm's hold on send, and
// the Not protected note, which opens by itself the first time a room opens on a matter in that state (explained.ts)
// and on a press of the line or the toolbar's mark. The note, the confirm and the firm's notice load lazily
// (note-load.ts) once the open matter is Not protected; the line, the mark and the hold are in the first load.
import { useDisplay } from "@clauseo/components/lib/display.ts";
import { useMemo, useState, useSyncExternalStore, type ReactNode } from "react";
import { protectionStoreOf } from "../../store/protection.ts";
import type { HostTransport } from "../../transport/host-transport.ts";
import type { MatterInfo } from "../host-next.ts";
import { notProtectedCause } from "./causes.ts";
import type { ComposerProtection } from "./context.ts";
import { protectionLine, type PlanKind } from "./line.tsx";
import { LazyPart } from "./note-load.ts";

export type RoomProtection = {
  /** Opens the note from the toolbar's Not protected mark; undefined while the open matter isn't Not protected. */
  explain: (() => void) | undefined;
  composer: ComposerProtection;
  /** The note, mounted once the open matter is Not protected. */
  note: ReactNode;
};

export function useRoomProtection(
  transport: HostTransport,
  open: MatterInfo | null,
  plan: PlanKind,
): RoomProtection {
  // memoised by hand: the composer's value alone needs to hold still, and the Compiler's cache would double this hook
  "use no memo";
  const { lang } = useDisplay();
  const [store] = useState(() => protectionStoreOf(transport));
  const matter = open?.matter ?? null;
  // the matter's row carries its protection now (matters.ts takes protection_changed onto it)
  const protection = open?.protection;
  // only the firm's requirement re-renders the room from the store
  const readRequired = () => store.state().view?.required === true;
  const required = useSyncExternalStore(store.subscribe, readRequired, readRequired);
  const cause = useMemo(() => notProtectedCause(protection), [protection]);
  const group = cause?.group ?? null;
  const [noteFor, setNoteFor] = useState<string | null>(null);
  const key = matter === null || group === null ? null : `${matter} ${group}`;
  const noteOpen = matter !== null && noteFor === matter;
  const explain = matter === null || cause === null ? undefined : () => setNoteFor(matter);
  const blocked = required && cause !== null;
  const composer = useMemo<ComposerProtection>(
    () => ({
      line: protectionLine(lang, protection, plan, () => setNoteFor(matter)),
      blocked,
      notice:
        blocked && cause !== null ? (
          <LazyPart part="required" transport={transport} store={store} cause={cause} />
        ) : null,
    }),
    [lang, protection, plan, matter, blocked, cause, transport, store],
  );
  return {
    explain,
    composer,
    note:
      cause === null || open === null ? null : (
        <LazyPart
          part="note"
          key={key}
          matterId={open.matter}
          transport={transport}
          store={store}
          matter={open.name}
          cause={cause}
          open={noteOpen}
          onClose={() => setNoteFor(null)}
        />
      ),
  };
}
