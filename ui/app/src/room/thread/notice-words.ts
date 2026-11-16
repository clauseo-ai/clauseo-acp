// The app's own line for a host notice, by its kind (NoticeEntry.kind): the words the lawyer reads in place of the
// host's text. An agent's notice has no kind; a kind this version doesn't know (a newer host's, arriving as some other
// string or as an object it doesn't read) gives null, and the row shows the notice's text as the host wrote it.
import { listOf, namesOf } from "@clauseo/components/lib/words-core.ts";
import type { useThreadWords } from "@clauseo/components/lib/words-thread.ts";
import type { AgentName } from "@clauseo/components/lib/words-errors.ts";
import type { NoticeKind } from "../../transport/protocol.ts";
import { agentNameOf } from "./stops.ts";

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

/** The line for a step Clauseo's pre-write guard stopped (the host's held_open_blocked with its paper's name,
 * codex_folder_blocked, guard_step_blocked with no paper named, or guard_steps_more counting the turn's steps past
 * those told): the agent named as the outside-matter line names it, the AI where none is known; the paper named as
 * Versions names it. A kind of another shape gives null. */
function guardBlocked(kind: NoticeKind, w: NoticeWords, agent?: AgentName): string | null {
  if (kind === "guard_step_blocked") return w.th("Clauseo stopped a step.");
  const more =
    typeof kind === "object" && kind !== null && "guard_steps_more" in kind
      ? kind.guard_steps_more
      : undefined;
  if (more !== undefined) {
    if (typeof more.more !== "number" || more.more < 1) return null;
    return more.more === 1
      ? w.th("Clauseo stopped one more step.")
      : w.th("Clauseo stopped {n} more steps.", { n: more.more });
  }
  const name = agentNameOf(agent);
  if (kind === "codex_folder_blocked")
    return name === null
      ? w.th(
          "Clauseo stopped a step: the AI tried to change its own settings in this matter's folder.",
        )
      : w.th(
          "Clauseo stopped a step: {name} tried to change its own settings in this matter's folder.",
          {
            name,
          },
        );
  const held =
    typeof kind === "object" && kind !== null && "held_open_blocked" in kind
      ? kind.held_open_blocked
      : undefined;
  if (held === undefined || typeof held.file !== "string") return null;
  const file = namesOf([held.file])(held.file);
  return name === null
    ? w.th(
        "Clauseo stopped a step: the AI tried to change {file}, which is open in Word. Close it in Word, then ask again.",
        { file },
      )
    : w.th(
        "Clauseo stopped a step: {name} tried to change {file}, which is open in Word. Close it in Word, then ask again.",
        { name, file },
      );
}

/** The line for a notice's kind in the page's language, its slots filled; null for no kind, or a kind this version
 * does not know, so the notice's own text shows. `agent`: the open conversation's agent
 * (useAgentPlan), named as the outside-matter stop names it (stops.ts). */
export function noticeLine(
  kind: NoticeKind | undefined,
  w: NoticeWords,
  agent?: AgentName,
): string | null {
  if (kind === "outside_matter_blocked") {
    const name = agentNameOf(agent);
    return name === null
      ? w.th("Clauseo stopped a step: the AI tried to reach outside this matter.")
      : w.th("Clauseo stopped a step: {name} tried to reach outside this matter.", { name });
  }
  if (kind === "shell_network_blocked") {
    const name = agentNameOf(agent);
    return name === null
      ? w.th(
          "Clauseo stopped a step: the AI tried to go online by running a command. It can use web search instead.",
        )
      : w.th(
          "Clauseo stopped a step: {name} tried to go online by running a command. {name} can use web search instead.",
          { name },
        );
  }
  // the same, in a session without web search: no web search is offered
  if (kind === "shell_network_blocked_no_web_search") {
    const name = agentNameOf(agent);
    return name === null
      ? w.th("Clauseo stopped a step: the AI tried to go online by running a command.")
      : w.th("Clauseo stopped a step: {name} tried to go online by running a command.", { name });
  }
  const guard = kind === undefined ? null : guardBlocked(kind, w, agent);
  if (guard !== null) return guard;
  if (typeof kind === "string") {
    if (kind === "context_not_restored")
      return w.th(
        "Clauseo doesn't remember the earlier questions and answers in this conversation. Include what it needs from them in your next question.",
      );
    if (kind === "auto_mode_unavailable")
      return w.th(
        "Claude's automatic mode isn't available for this account, so Claude asks before running commands.",
      );
    if (kind === "ask_mode_too_many_matters")
      return w.th(
        "Claude will ask before each change in this conversation. Clauseo is protecting more matters than Claude's automatic checks can handle at once, so Claude asks you instead.",
      );
    if (kind === "ask_mode_checks_stopped")
      return w.th(
        "Claude now asks before each change in this conversation, because its automatic checks stopped working here. Clauseo's own protections still apply.",
      );
    if (kind === "safety_check_no_answer")
      return w.th(
        "Claude's safety check didn't answer, so Claude didn't do this. Try again in a moment.",
      );
    return null;
  }
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
