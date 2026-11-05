// The app's own line for a host notice, by its kind (NoticeEntry.kind): the words the lawyer reads in place of the
// host's text. An agent's notice has no kind; a kind this version doesn't know (a newer host's, arriving as some other
// string or as an object it doesn't read) gives null, and the row shows the notice's text as the host wrote it.
import { listOf, namesOf } from "@clauseo/components/lib/words-core.ts";
import type { useThreadWords } from "@clauseo/components/lib/words-thread.ts";
import type { NoticeKind } from "../../transport/protocol.ts";

/** The thread's words the line is drawn with: its page language, the thread's own, and the shared ones ({n} more). */
export type NoticeWords = Pick<ReturnType<typeof useThreadWords>, "lang" | "th" | "t">;

/** "1 paper came back after the undo: ..." or "{n} papers came back ...": the papers named as Versions names files in
 * its dialogs (namesOf), joined by listOf; the shared "{n} more" ends the list when more came back than are named. */
function cameBack(files: readonly string[], more: number, w: NoticeWords): string {
  const n = files.length + more;
  const name = namesOf(files);
  const listed = files.map(name);
  if (more > 0) listed.push(w.t("{n} more", { n: more }));
  const papers = listOf(w.lang, listed);
  return n === 1
    ? w.th("1 paper came back after the undo: {papers}. Clauseo didn't remove it again.", {
        papers,
      })
    : w.th("{n} papers came back after the undo: {papers}. Clauseo didn't remove them again.", {
        n,
        papers,
      });
}

/** The line for a skill pack the launch left out, by its reason: the pack's id in straight quotes, as the app quotes
 * its other names (the id goes in as it is, as a thread's title does). A reason this version does not know gives null. */
function packSkipped(pack: string, reason: string, w: NoticeWords): string | null {
  switch (reason) {
    case "not_turned_on":
      return w.th(
        "Clauseo didn't load the skill pack '{pack}'. This matter is set to use it, but it isn't turned on.",
        { pack },
      );
    case "not_admitted":
      return w.th(
        "Clauseo didn't load the skill pack '{pack}'. The skill pack settings on this computer don't allow it.",
        { pack },
      );
    case "changed":
      return w.th(
        "Clauseo didn't load the skill pack '{pack}'. Its files changed after it was turned on. Turn it on again to use it.",
        { pack },
      );
    case "missing":
      return w.th(
        "Clauseo didn't load the skill pack '{pack}'. Clauseo couldn't find it on this computer.",
        { pack },
      );
    case "invalid":
      return w.th(
        "Clauseo didn't load the skill pack '{pack}'. Its files aren't a valid skill pack.",
        { pack },
      );
    case "too_many":
      return w.th(
        "Clauseo didn't load the skill pack '{pack}'. Clauseo loads at most 16 skill packs at a time.",
        { pack },
      );
    default:
      return null;
  }
}

/** The line for a notice's kind in the page's language, its slots filled; null for no kind, or a kind this version
 * does not know, so the notice's own text shows. */
export function noticeLine(kind: NoticeKind | undefined, w: NoticeWords): string | null {
  if (typeof kind === "string")
    return kind === "context_not_restored"
      ? w.th(
          "Clauseo doesn't remember the earlier questions and answers in this conversation. Include what it needs from them in your next question.",
        )
      : null;
  // read by shape: a value from a newer host, or none (null), that this version doesn't know gives null. The field
  // is read rather than trusted to `in`: beside another object kind, each variant marks the other's key optional
  const came =
    typeof kind === "object" && kind !== null && "files_came_back" in kind
      ? kind.files_came_back
      : undefined;
  if (came !== undefined)
    return came.files.length + came.more === 0 ? null : cameBack(came.files, came.more, w);
  const skipped =
    typeof kind === "object" && kind !== null && "skill_pack_not_loaded" in kind
      ? kind.skill_pack_not_loaded
      : undefined;
  if (skipped === undefined || typeof skipped.pack !== "string") return null;
  return packSkipped(skipped.pack, skipped.reason, w);
}
