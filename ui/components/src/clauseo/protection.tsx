// Not protected, explained. When a matter stands without protection, the system's alert dialog says why in plain
// lines: what Clauseo can now reach, and what is still kept. The cause picks the body (UC): protection not set up on
// this computer yet, the set-up refused or failed, a computer that can't give it, every protected space in use (counts
// only, never a name), or protection switched off for the matter. The body is the same for both plans: "asks before
// changing anything outside this matter's folder" is true for Claude and ChatGPT alike. Understood, the act, is the
// dark primary and takes the starting focus. A set-up cause offers its retry beside it ("Set up protection", "Try
// again"; a full pool's "Add room" while it can grow), which leaves the note open: the waiting line stands in the
// button's place, and a refusal's words under it. The open lock leads it, in concrete: nothing here is red.
import { AlertDialog, AlertDialogClose, AlertDialogPopup } from "../ui/alert-dialog.tsx";
import { Button } from "../ui/button.tsx";
import { IconNotProtected } from "../icons/icons.tsx";
import { useDisplay } from "../lib/display.ts";
import { pw } from "../lib/words-protection.ts";
import { useVersionWords } from "../lib/words-versions.ts";
import { Say } from "./say.tsx";

/** Why the matter is not protected, as the note says it. "setup" is the older name of "set_up_refused". */
export type NoteWhy =
  | "off"
  | "setup"
  | "needs_set_up"
  | "set_up_refused"
  | "set_up_failed"
  | "cant"
  | "not_built"
  | "pool";

/** The pool's counts (NotProtectedCause's pool_full): how many protected spaces, and what holds them. */
export type NotePool = {
  size: number;
  open: number;
  held: number;
  owed: number;
  set_aside: number;
};

export function NotProtectedNote({
  open,
  matter,
  why,
  pool,
  named1385 = false,
  addRoom = false,
  waiting = null,
  failed = null,
  onClose,
  onProtect,
}: {
  open: boolean;
  /** The matter's name as filed. */
  matter: string;
  why: NoteWhy;
  /** The pool's counts, for "pool". */
  pool?: NotePool | undefined;
  /** Optional: R17 PR-E names Windows error 1385 inside a failed set-up; its line takes the platform line's place. */
  named1385?: boolean;
  /** Optional, for "pool": the pool can grow, so "Add room" shows. */
  addRoom?: boolean;
  /** Optional: the waiting line, in the retry's place while its command runs. */
  waiting?: string | null;
  /** Optional: the last retry's refusal, in its code's words. */
  failed?: string | null;
  onClose: () => void;
  /** The retry: set protection up, try again, or add room. Without it, no retry shows. */
  onProtect?: (() => void) | undefined;
}) {
  const w = useVersionWords();
  const { platform } = useDisplay();
  const p = (key: Parameters<typeof pw>[1], slots?: Parameters<typeof pw>[2]) =>
    pw(w.lang, key, slots);
  const refused = why === "setup" || why === "set_up_refused";
  const counts: [number, string][] =
    why === "pool" && pool !== undefined
      ? [
          [pool.open, p("In use by other open matters: {n}", { n: pool.open })],
          [pool.held, p("Kept by matters with nothing open: {n}", { n: pool.held })],
          [pool.owed, p("Freed the next time Clauseo starts work: {n}", { n: pool.owed })],
          [pool.set_aside, p("Still holding files set aside: {n}", { n: pool.set_aside })],
        ]
      : [];
  const lines: string[] =
    why === "off"
      ? [
          w.v(
            "Protection is off for {matter}. Clauseo can read and change files anywhere on this computer while it works here, without asking first.",
            { matter },
          ),
        ]
      : why === "needs_set_up"
        ? [
            p(
              "Protection isn't set up on this computer yet. Until it is, Clauseo can read any file on this computer, and asks before changing anything outside this matter's folder.",
            ),
          ]
        : why === "not_built"
          ? [
              p(
                "This build of Clauseo comes without protection, so Clauseo can read any file on this computer. It asks before changing anything outside this matter's folder.",
              ),
            ]
          : why === "cant"
            ? [
                p(
                  "This computer can't give Clauseo's protection, so Clauseo can read any file on it. It asks before changing anything outside this matter's folder.",
                ),
              ]
            : why === "pool"
              ? [
                  p(
                    "All {size} of this computer's protected spaces are in use, so this matter works without protection for now. It's protected again once one is free.",
                    { size: pool?.size ?? 0 },
                  ),
                ]
              : [
                  p(
                    "Protection couldn't be set up on this computer, so Clauseo can read any file on it. It asks before changing anything outside this matter's folder.",
                  ),
                  ...(refused
                    ? [
                        platform === "windows"
                          ? w.v("Windows didn't allow the protected space on this computer.")
                          : platform === "linux"
                            ? w.v("Linux didn't allow the protected space on this computer.")
                            : w.v("macOS didn't allow the protected space on this computer."),
                      ]
                    : named1385
                      ? [
                          p(
                            "Windows policy on this computer doesn't let Clauseo's protection accounts sign in. Ask your IT team to add them to the Allow log on locally policy.",
                          ),
                        ]
                      : []),
                ];
  const retry =
    onProtect === undefined ||
    why === "off" ||
    why === "cant" ||
    why === "not_built" ||
    (why === "pool" && !addRoom)
      ? null
      : why === "needs_set_up"
        ? p("Set up protection")
        : why === "pool"
          ? p("Add room")
          : w.v("Try again");
  return (
    <AlertDialog open={open} onOpenChange={(o) => !o && onClose()}>
      <AlertDialogPopup
        undoable
        className="np-note"
        data-testid="np-note"
        data-cause={why === "setup" ? "set_up_refused" : why}
        title={
          <span className="np-note-title">
            <span className="card-icon np-note-icon">
              <IconNotProtected size={18} />
            </span>
            <span>{w.v("This matter is not protected")}</span>
          </span>
        }
        body={
          <span className="np-note-body">
            {lines.map((line) => (
              <span key={line}>
                <Say>{line}</Say>
              </span>
            ))}
            {/* a list, by role: the body is the dialog's description, a <p>, which holds no <ul> */}
            {counts.some(([n]) => n > 0) && (
              <span className="np-note-counts" role="list" data-testid="np-pool">
                {counts
                  .filter(([n]) => n > 0)
                  .map(([, line]) => (
                    <span key={line} role="listitem">
                      <Say>{line}</Say>
                    </span>
                  ))}
              </span>
            )}
            {/* the ways out: ending a conversation frees a space another open matter holds; under the largest pool,
                Add room makes 4 more behind Windows' prompt */}
            {why === "pool" && ((pool?.open ?? 0) > 0 || addRoom) && (
              <span data-testid="np-pool-out">
                <Say>
                  {[
                    ...((pool?.open ?? 0) > 0
                      ? [p("To free one now, end a conversation in another matter.")]
                      : []),
                    ...(addRoom
                      ? [p("Clauseo can add room for 4 more. Windows asks for permission first.")]
                      : []),
                  ].join(" ")}
                </Say>
              </span>
            )}
            <span>
              <Say>
                {w.v(
                  "Every change inside the matter's folder is still kept, and Versions can undo it.",
                )}
              </Say>
            </span>
            {failed !== null && (
              <span className="np-note-failed" role="status" data-testid="protection-failed">
                <Say>{failed}</Say>
              </span>
            )}
          </span>
        }
        actions={
          <>
            {retry !== null &&
              (waiting !== null ? (
                <span className="np-note-wait" role="status" data-testid="protection-wait">
                  <Say>{waiting}</Say>
                </span>
              ) : (
                <Button kind="quiet" onClick={onProtect} data-testid="protection-setup">
                  <span className="deva-nudge">{retry}</span>
                </Button>
              ))}
            <AlertDialogClose render={<Button kind="primary" />}>
              <span className="deva-nudge">{w.v("Understood")}</span>
            </AlertDialogClose>
          </>
        }
      />
    </AlertDialog>
  );
}
