// Versions in the room, from its own chunk: the timeline in the rail's place (VersionsRail), and the band over the
// work with the restore dialog (VersionsCentre). Both read one VersionsController (controller.ts) and compose the parts
// of ui/components: VersionPanel, VersionWhere, VersionBand, CompareBar, PdfSheet and RestoreDialog.
import { useEffect, useId, useMemo, useRef, useState, useSyncExternalStore } from "react";
import { RestoreDialog } from "@clauseo/components/clauseo/restore-dialog.tsx";
import { keyIsMine, useRoom } from "@clauseo/components/clauseo/room-scope.ts";
import {
  CompareBar,
  VersionBand,
  WaitingButton,
  type BandNotice,
} from "@clauseo/components/clauseo/version-band.tsx";
import { Desk, PdfSheet } from "@clauseo/components/clauseo/version-page.tsx";
import { VersionPanel } from "@clauseo/components/clauseo/version-panel.tsx";
import { VersionWhere } from "@clauseo/components/clauseo/version-where.tsx";
import {
  asItStood,
  didOf,
  whoOf,
  type Calendar,
  type Version,
} from "@clauseo/components/clauseo/versions-model.ts";
import { IconUndo } from "@clauseo/components/icons/icons.tsx";
import { Button } from "@clauseo/components/ui/button.tsx";
import type { Lang, Platform } from "@clauseo/components/lib/display.ts";
import { useVersionWords, vt } from "@clauseo/components/lib/words-versions.ts";
import type { FileRefId } from "../../transport/protocol.ts";
import { isHostError } from "../../transport/host-transport.ts";
import type { VersionsController } from "./controller.ts";
import { restoreOf, type Snapshot } from "./state.ts";
import type { StandIns } from "./stand-ins.ts";
import { calendarOf, fileName, undoable } from "./timeline.ts";
import {
  askOf,
  conversationOf,
  doneLine,
  failedLine,
  named,
  unsavedAt,
  unsavedLine,
  waitingOf,
} from "./words.ts";

/** A file's handle in the matter's papers (H8), for H11's open_file; null when the room has none for it. */
export type FileRefOf = (path: string) => FileRefId | null;

function useVersions(
  c: VersionsController,
  lang: Lang,
): { s: Snapshot; cal: Calendar; history: readonly Version[] } {
  const s = useSyncExternalStore(c.subscribe, c.snapshot, c.snapshot);
  const cal = useMemo(() => calendarOf(s.now, s.zone), [s.now, s.zone]);
  const history = useMemo(() => named(lang, s.timeline.history), [lang, s.timeline.history]);
  return { s, cal, history };
}

/** A stand-in's refusal is logged; the lawyer declining the computer's own confirm says nothing. */
const quietly = (run: Promise<void>) =>
  run.catch((error: unknown) => {
    if (isHostError(error) && error.code === "transport.unauthorized") return;
    console.warn(`a Versions act failed: ${JSON.stringify(error)}`);
  });

/** The timeline, where the rail stands while Versions is open. */
export function VersionsRail({
  controller: c,
  platform,
  standIns,
  onClose,
  onPicked,
  focusOnOpen,
}: {
  controller: VersionsController;
  platform: Platform;
  standIns: StandIns;
  onClose: () => void;
  /** After a pick: on a mid window, the panel goes away. */
  onPicked: () => void;
  /** Brought back over the work on a mid window: the panel takes the focus as it opens. */
  focusOnOpen: boolean;
}) {
  const w = useVersionWords();
  const { s, cal, history } = useVersions(c, w.lang);
  const { meta } = s.timeline;
  const look = s.look;
  const at = look.kind === "back" ? history.findIndex((v) => v.id === look.id) : history.length - 1;
  const [whereOpen, setWhereOpen] = useState(false);
  const [where, setWhere] = useState<{ folder?: string; size?: string }>({});
  const { reveal, versionsFolder, diskUse } = standIns;
  useEffect(() => {
    let live = true;
    // H12's folder and size, where a host gives them
    void Promise.all([versionsFolder?.(c.matter), diskUse?.(c.matter)]).then(
      ([folder, size]) => {
        if (live)
          setWhere({
            ...(folder === undefined ? {} : { folder }),
            ...(size === undefined ? {} : { size }),
          });
      },
      (error: unknown) => console.warn(`the versions' folder or size: ${JSON.stringify(error)}`),
    );
    return () => {
      live = false;
    };
  }, [c, versionsFolder, diskUse]);
  const u = s.unsaved;
  return (
    <VersionPanel
      history={history}
      calendar={cal}
      name={fileName}
      at={at < 0 ? history.length - 1 : at}
      onPick={(i) => {
        c.look(history[i]?.id ?? null);
        onPicked();
      }}
      onClose={onClose}
      unsaved={
        u === null
          ? null
          : {
              ...unsavedAt(w.lang, s, u),
              who: vt(w.lang, "Clauseo, in {conversation}", {
                conversation: conversationOf(w.lang, s, u.thread),
              }),
              why: vt(w.lang, "Not kept as a version."),
            }
      }
      where={
        <VersionWhere
          platform={platform}
          open={whereOpen}
          onOpenChange={setWhereOpen}
          {...where}
          {...(reveal === undefined ? {} : { onShow: () => void quietly(reveal("app_data")) })}
        />
      }
      testIds={{ panel: "versions", entry: "version" }}
      focusOnOpen={focusOnOpen}
      kindOf={(v) => meta.get(v.id)?.kind ?? "unknown"}
    />
  );
}

/** Whether a version's change can be taken back from the band: a turn's, with undo_step, or a restore, by restoring
 * the version before it through the restore dialog. */
const takesBack = (s: Snapshot, v: Version): boolean => {
  const m = s.timeline.meta.get(v.id);
  return undoable(m) || m?.kind === "restore";
};

/** The band over the work, the comparison under it, and the restore dialog. */
export function VersionsCentre({
  controller: c,
  standIns,
  fileRef,
  onLeave,
  onPanel,
  onAway,
}: {
  controller: VersionsController;
  standIns: StandIns;
  fileRef: FileRefOf;
  /** Closes Versions: Cancel in a dialog the toolbar's Undo opened along with Versions. */
  onLeave: () => void;
  /** With the panel put away on a mid window: brings it back, from the band's Versions. */
  onPanel: (() => void) | null;
  /** With the panel over the band on a mid window: puts it away, Escape's first step from inside it. */
  onAway: (() => void) | null;
}) {
  const w = useVersionWords();
  const { lang } = w;
  const room = useRoom();
  const { s, cal, history } = useVersions(c, lang);
  const look = s.look;
  // looking back at a version, or at now with the newest picked: a turn's change whose Undo the band offers
  const picked = look.id === undefined ? undefined : history.find((v) => v.id === look.id);
  const atNewest = look.kind === "now" && picked !== undefined && takesBack(s, picked);
  const shown = look.kind === "back" || atNewest ? picked : undefined;
  const waiting = waitingOf(lang, s);
  // the line that says Restore and Undo wait, which every waiting act points to
  const waitId = useId();
  const desk = useRef<HTMLDivElement>(null);
  const [doc, setDoc] = useState<string | null>(null);
  const lead = useRef<HTMLButtonElement>(null);
  const away = onPanel !== null;
  // the panel put away took the focus with it: the band's Versions, which brings it back, takes it
  useEffect(() => {
    const at = document.activeElement;
    if (away && (at === null || at === document.body)) lead.current?.focus();
  }, [away]);

  // Escape comes back to now, from anywhere in the room but a field or a dialog; while a panel stands over the band,
  // wherever the focus is, it puts the panel away first and keeps the look
  useEffect(() => {
    if (shown === undefined || s.ask !== null) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || e.altKey || e.ctrlKey || e.metaKey || e.shiftKey) return;
      const t = e.target;
      if (
        t instanceof HTMLElement &&
        (t.isContentEditable || t instanceof HTMLInputElement || t instanceof HTMLTextAreaElement)
      )
        return;
      if (!keyIsMine(room, t)) return;
      e.preventDefault();
      if (onAway !== null) onAway();
      else c.look(null);
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [c, shown, s.ask, room, onAway]);

  const notices: BandNotice[] = [];
  if (look.kind === "now") {
    if (s.unsaved !== null) notices.push({ tone: "failed", text: unsavedLine(lang, s, s.unsaved) });
    const done = s.done;
    // the restored line's Undo, once the host's marker says the restore was written
    const restore = done === null ? null : restoreOf(s, done);
    if (done !== null)
      notices.push({
        tone: "done",
        text: doneLine(lang, cal, s, done),
        ...(restore !== null
          ? {
              action: (
                <WaitingButton
                  waiting={waiting !== null}
                  describedBy={waitId}
                  kind="quiet"
                  className="ver-notice-act ver-restored-undo"
                  label={w.v("Undo the restore")}
                  onClick={() => c.undoRestore(restore)}
                >
                  <IconUndo size={16} />
                  {w.t("Undo")}
                </WaitingButton>
              ),
            }
          : {}),
      });
  }
  // a refusal says why on the page, by its code: under the band at now, as the band's note looking back; a running
  // answer says so in the waiting line instead
  const failure =
    s.failed !== null && s.failed !== "matter.turn_running" && s.failed !== "thread.turn_running"
      ? failedLine(lang, s.failed)
      : null;
  if (failure !== null) notices.push(failure);

  const changes = s.compare?.changes ?? null;
  const inView = changes?.find((f) => f.path === doc) ?? changes?.[0];
  const ask = askOf(lang, cal, s, s.ask);
  const held =
    s.ask?.act === "undo" &&
    s.ask.stage === "confirm" &&
    ![...s.ask.choices.values()].includes("undo");
  return (
    <div className="ver-room-band">
      <VersionBand
        look={
          shown === undefined
            ? null
            : {
                line: atNewest ? w.v("Now") : asItStood(lang, cal, shown),
                now: atNewest,
                who: whoOf(lang, cal, shown, history),
                did: didOf(lang, shown, fileName),
                comparing: look.kind === "back" && look.comparing,
                undoable: takesBack(s, shown),
                note: failure?.text ?? null,
              }
        }
        notices={notices}
        {...(onPanel === null
          ? {}
          : {
              lookLead: (
                <Button
                  ref={lead}
                  kind="quiet"
                  size="small"
                  className="ver-open-panel"
                  aria-expanded={false}
                  onClick={onPanel}
                >
                  {w.v("Versions")}
                  <span className="ver-open-count">{history.length}</span>
                </Button>
              ),
            })}
        waiting={waiting}
        onCompare={() => c.toggleCompare()}
        onRestore={() => shown !== undefined && void c.askRestore(shown.id)}
        onUndo={() => {
          if (shown === undefined) return;
          if (s.timeline.meta.get(shown.id)?.kind === "restore") c.undoRestore(shown.id);
          else c.askUndo(shown.id);
        }}
        onBack={() => c.look(null)}
        testIds={{ restore: "version-restore", undo: "version-undo", wait: "versions-wait" }}
        waitId={waitId}
      />
      {look.kind === "back" && look.comparing && changes !== null && shown !== undefined && (
        <>
          <CompareBar
            docs={changes.map((f) => ({ id: f.path, label: fileName(f.path) }))}
            doc={inView?.path ?? ""}
            onDoc={setDoc}
            legend={changes.length === 0 ? vt(lang, "No changes since then.") : ""}
            unchanged=""
          />
          {inView !== undefined && (
            <Desk label={fileName(inView.path)} scrollRef={desk}>
              <PdfSheet
                title={fileName(inView.path)}
                changed
                state={
                  shown.day === cal.today
                    ? vt(lang, "Changed since {time} today", { time: w.clock(shown.at) })
                    : vt(lang, "Changed since {time} on {date}", {
                        time: w.clock(shown.at),
                        date: cal.words(lang, shown.day, "date"),
                      })
                }
                then={{
                  head:
                    shown.day === cal.today
                      ? vt(lang, "As it was at {time} today", { time: w.clock(shown.at) })
                      : vt(lang, "As it was at {time} on {date}", {
                          time: w.clock(shown.at),
                          date: cal.words(lang, shown.day, "date"),
                        }),
                  facts: "",
                  saved: "",
                }}
                now={{ head: vt(lang, "As it is now"), facts: "", saved: "" }}
                pdf={/\.pdf$/i.test(inView.path)}
                {...openNow(standIns, fileRef(inView.path))}
              />
            </Desk>
          )}
        </>
      )}
      <RestoreDialog
        ask={ask}
        waiting={waiting}
        held={held}
        onClose={() => c.close() && onLeave()}
        onChoice={(id, choice) => c.choose(id, choice)}
        onRestore={() => void c.confirm()}
        onTryAgain={() => void c.tryAgain()}
        testIds={{ confirm: "dialog-confirm", tryAgain: "dialog-try-again" }}
      />
    </div>
  );
}

/** "Open now", through H11's open_file, where the host has it and the room has the file's handle. The file as it was
 * has no handle until the matter as it stood can be read (H12), so it has no way to open. */
function openNow(standIns: StandIns, file: FileRefId | null) {
  const { openFile } = standIns;
  if (openFile === undefined || file === null) return {};
  return {
    openable: "now" as const,
    onOpen: (which: "then" | "now") => {
      if (which === "now") void quietly(openFile(file));
    },
  };
}
