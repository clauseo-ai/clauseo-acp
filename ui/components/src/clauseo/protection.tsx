// Not protected, explained. When a matter stands without protection, the system's alert dialog says
// why in plain lines: what Clauseo can now reach, and what is still kept. The cause picks the body
// (UC): protection not set up on this computer yet, the set-up refused or failed, a computer that
// can't give it, every protected space in use (counts only, never a name), or protection switched
// off for the matter. The body is the same for both plans: "asks before changing anything outside
// this matter's folder" is true for Claude and ChatGPT alike. Design B adds a Safe workspace
// something on this computer stops, whose reason's line the caller gives, and the repair: its line,
// and on a personal install "Get the installer", which opens the installed version's release page
// (the caller's act). Understood, the act, is the dark primary and takes the starting focus. A
// set-up cause offers its retry beside it ("Set up the Safe workspace", "Try again"; a full pool's
// "Add room" while it can grow), which leaves the note open: the waiting line stands in the
// button's place, and a refusal's words under it. R17 PR-E: a full pool refuses once tier B is in use,
// so its body says Clauseo can't answer here, by the sandbox's why (no free place, or two
// conversations already working in this matter, which offers no pool action), with the reset line
// and "Free up places" where places need a reset; an item a place didn't hand back (pool_owned)
// says so with the IT line, naming where when known. The open lock leads it, in concrete: nothing
// here is red. R17 PR-E's M4: a set-up in place but broken, a block, and a cause this version
// doesn't know refuse every launch, so their bodies say Clauseo can't answer here, with no retry. #722's probe_failed
// refuses too, and its Try again checks again.
import { AlertDialog, AlertDialogClose, AlertDialogPopup } from "../ui/alert-dialog.tsx";
import { Button } from "../ui/button.tsx";
import { IconNotProtected } from "../icons/icons.tsx";
import { useDisplay } from "../lib/display.ts";
import { ownershipLine } from "../lib/words-errors.ts";
import { INSTALLER_CLOSE, pw } from "../lib/words-protection.ts";
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
  | "pool"
  | "pool_owned"
  | "blocked"
  | "unknown"
  | "probe_failed";

/** The pool's counts (NotProtectedCause's pool_full): how many protected spaces, and what holds them. */
export type NotePool = {
  size: number;
  open: number;
  held: number;
  owed: number;
  set_aside: number;
  /** Optional: the places only a reset frees (R17 PR-E's needs_reset; 0 before PR-D). */
  needs_reset?: number | undefined;
  /** Optional: why no place was free, as the sandbox names it; anything else (or none) reads as the plain full pool. */
  why?: string | null | undefined;
};

/** The pool's body by its why: no free place, two conversations already in this matter, or the plain full pool. */
const poolKind = (pool: NotePool | undefined): "no_free_slot" | "matter_sessions_full" | "full" =>
  pool?.why === "no_free_slot" || pool?.why === "matter_sessions_full" ? pool.why : "full";

export function NotProtectedNote({
  open,
  matter,
  why,
  pool,
  addRoom = false,
  waiting = null,
  failed = null,
  extra = [],
  where = null,
  onClose,
  onProtect,
  onFreeUp,
  onGetInstaller,
}: {
  open: boolean;
  /** The matter's name as filed. */
  matter: string;
  why: NoteWhy;
  /** The pool's counts, for "pool". */
  pool?: NotePool | undefined;
  /** Optional, for "pool": the pool can grow, so "Add room" shows. */
  addRoom?: boolean;
  /** Optional: the waiting line, in the retry's place while its command runs. */
  waiting?: string | null;
  /** Optional: the last retry's refusal, in its code's words. */
  failed?: string | null;
  /** Optional: lines after the cause's body, in the page's language (design B: the block's reason,
   * the repair). */
  extra?: string[];
  /** Optional, for "pool_owned": the item's IT line names it (a refused turn's `at`); without it, "it". */
  where?: string | null;
  onClose: () => void;
  /** Optional, for "pool": frees the places only a reset frees (`retry_protection`, behind Windows' prompt); shown
   * only while the pool's `needs_reset` is above 0. */
  onFreeUp?: (() => void) | undefined;
  /** The retry: set protection up, try again, or add room. Without it, no retry shows. */
  onProtect?: (() => void) | undefined;
  /** Optional: opens the installer's page ("Get the installer"); without it, no such button. */
  onGetInstaller?: (() => void) | undefined;
}) {
  const w = useVersionWords();
  const { platform } = useDisplay();
  const p = (key: Parameters<typeof pw>[1], slots?: Parameters<typeof pw>[2]) =>
    pw(w.lang, key, slots);
  const refused = why === "setup" || why === "set_up_refused";
  const kind = poolKind(pool);
  // two conversations already in this matter: the pool's counts and ways out don't apply
  const poolWide = why === "pool" && kind !== "matter_sessions_full";
  const resets = poolWide ? (pool?.needs_reset ?? 0) : 0;
  const counts: [number, string][] =
    poolWide && pool !== undefined
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
            "The Safe workspace is off for {matter}. Clauseo can read and change files anywhere on this computer while it works here, without asking first.",
            { matter },
          ),
        ]
      : why === "needs_set_up"
        ? [
            p(
              "The Safe workspace isn't set up on this computer yet. Until it is, Clauseo can read any file you can open on this computer, and asks before changing anything outside this matter's folder.",
            ),
          ]
        : why === "probe_failed"
          ? [
              p(
                "Clauseo couldn't check the Safe workspace on this computer, so it can't answer here yet. Try again. If it keeps happening, restart Clauseo.",
              ),
            ]
          : why === "not_built"
            ? [
                p(
                  "This version of Clauseo doesn't have the Safe workspace yet. It comes in an update. Until then, Clauseo can read any file you can open on this computer, and asks before changing anything outside this matter's folder.",
                ),
              ]
            : why === "blocked"
              ? [
                  p(
                    "Something on this computer stops the Safe workspace, so Clauseo can't answer here.",
                  ),
                ]
              : why === "cant"
                ? [
                    p(
                      "This computer can't run the Safe workspace, so Clauseo can read any file you can open on it. It asks before changing anything outside this matter's folder.",
                    ),
                  ]
                : why === "pool"
                  ? [
                      kind === "matter_sessions_full"
                        ? p(
                            "Two conversations are already working in this matter. Try again when one finishes.",
                          )
                        : kind === "no_free_slot"
                          ? p(
                              "All {size} places in this computer's Safe workspace are in use by open conversations. End a conversation in another matter to free one.",
                              { size: pool?.size ?? 0 },
                            )
                          : p(
                              "All {size} places in this computer's Safe workspace are in use, so Clauseo can't answer in this matter until one is free. End a conversation in another matter to free one.",
                              { size: pool?.size ?? 0 },
                            ),
                    ]
                  : why === "pool_owned"
                    ? [
                        p(
                          "Part of this matter's folder wasn't handed back to you after the Safe workspace last used it, so Clauseo can't answer in this matter until it is.",
                        ),
                        ownershipLine(w.lang, where),
                      ]
                    : why === "set_up_failed"
                      ? [
                          p(
                            "The Safe workspace on this computer isn't working, so Clauseo can't answer here until it's set up again.",
                          ),
                        ]
                      : why === "unknown"
                        ? [
                            p(
                              "This version of Clauseo doesn't recognise what's stopping the Safe workspace, so it can't answer here. Update Clauseo, or ask your IT team.",
                            ),
                          ]
                        : [
                            p(
                              "The Safe workspace couldn't be set up on this computer, so Clauseo can read any file you can open on it. It asks before changing anything outside this matter's folder.",
                            ),
                            ...(refused
                              ? [
                                  platform === "windows"
                                    ? w.v(
                                        "Windows didn't allow the Safe workspace on this computer.",
                                      )
                                    : platform === "linux"
                                      ? w.v(
                                          "Linux didn't allow the Safe workspace on this computer.",
                                        )
                                      : w.v(
                                          "macOS didn't allow the Safe workspace on this computer.",
                                        ),
                                ]
                              : []),
                          ];
  // the line beside the installer's button: the button quits Clauseo once the page is open
  const close = onGetInstaller === undefined ? [] : [p(INSTALLER_CLOSE)];
  const retry =
    onProtect === undefined ||
    why === "off" ||
    why === "cant" ||
    why === "blocked" ||
    why === "pool_owned" ||
    why === "set_up_failed" ||
    why === "unknown" ||
    why === "not_built" ||
    (why === "pool" && (!addRoom || !poolWide))
      ? null
      : why === "needs_set_up"
        ? p("Set up the Safe workspace")
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
            {[...lines, ...extra, ...close].map((line) => (
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
            {poolWide && addRoom && (
              <span data-testid="np-pool-out">
                <Say>
                  {p("Clauseo can add room for 4 more. Windows asks for permission first.")}
                </Say>
              </span>
            )}
            {resets > 0 && (
              <span data-testid="np-pool-reset">
                <Say>
                  {resets === 1
                    ? p(
                        "One of them needs resetting before it can be used again. Windows asks for permission first.",
                      )
                    : p(
                        "{n} of them need resetting before they can be used again. Windows asks for permission first.",
                        { n: resets },
                      )}
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
            {onGetInstaller !== undefined && (
              <Button kind="quiet" onClick={onGetInstaller} data-testid="protection-installer">
                <span className="deva-nudge">{p("Get the installer")}</span>
              </Button>
            )}
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
            {resets > 0 &&
              onFreeUp !== undefined &&
              (waiting !== null ? (
                retry === null && (
                  <span className="np-note-wait" role="status" data-testid="protection-wait">
                    <Say>{waiting}</Say>
                  </span>
                )
              ) : (
                <Button kind="quiet" onClick={onFreeUp} data-testid="protection-free-up">
                  <span className="deva-nudge">{p("Free up places")}</span>
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
