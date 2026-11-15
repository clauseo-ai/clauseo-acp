// The app's own line for a host notice, by its kind (NoticeEntry.kind): the words the lawyer reads in place of the
// host's text. An agent's notice has no kind; a kind this version doesn't know (a newer host's, arriving as some other
// string or as an object it doesn't read) gives null, and the row shows the notice's text as the host wrote it.
import { isolate, listOf, namesOf } from "@clauseo/components/lib/words-core.ts";
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

/** A matter's name as the app's list has it now, by its id; undefined for a matter the list doesn't hold. */
export type MatterNameOf = (matter: string) => string | undefined;

const NO_NAMES: MatterNameOf = () => undefined;

/** The other matter a refusal hit, read by shape: its id, and its name now (the app's list), else as the host named it
 * then; undefined for anything else. */
export function refusedMatterOf(
  kind: NoticeKind | undefined,
  names: MatterNameOf = NO_NAMES,
): { matter: string; name: string } | undefined {
  const refused =
    typeof kind === "object" && kind !== null && "other_matter_refused" in kind
      ? kind.other_matter_refused
      : undefined;
  if (refused === undefined || typeof refused.matter !== "string") return undefined;
  const name = names(refused.matter) ?? (typeof refused.name === "string" ? refused.name : "");
  return { matter: refused.matter, name };
}

/** "Clauseo stopped a step: Claude tried to open Sharma's files. That's another matter.": the matter isolated, as a
 * name the lawyer typed. */
function otherRefused(matter: string, w: NoticeWords, agent: AgentName): string {
  const name = agentNameOf(agent);
  const slots = { matter: isolate(matter) };
  return name === null
    ? w.th(
        "Clauseo stopped a step: the AI tried to open {matter}'s files. That's another matter.",
        slots,
      )
    : w.th(
        "Clauseo stopped a step: {name} tried to open {matter}'s files. That's another matter.",
        {
          ...slots,
          name,
        },
      );
}

/** "Clauseo couldn't fully keep Claude out of these matters: A, B and 3 more.": each matter by its name now, a
 * matter the app's list doesn't hold counted with the more. Null when none can be named, so the host's text shows. */
function unfenced(
  matters: readonly string[],
  more: number,
  w: NoticeWords,
  names: MatterNameOf,
  agent: AgentName,
): string | null {
  const named = matters.flatMap((m) => {
    const name = names(m);
    return name === undefined ? [] : [isolate(name)];
  });
  if (named.length === 0) return null;
  const rest = more + matters.length - named.length;
  if (rest > 0) named.push(w.t("{n} more", { n: rest }));
  const list = listOf(w.lang, named);
  const name = agentNameOf(agent);
  return name === null
    ? w.th("Clauseo couldn't fully keep the AI out of these matters: {matters}.", {
        matters: list,
      })
    : w.th("Clauseo couldn't fully keep {name} out of these matters: {matters}.", {
        name,
        matters: list,
      });
}

/** The line for a notice's kind in the page's language, its slots filled; null for no kind, or a kind this version
 * does not know, so the notice's own text shows. `agent`: the open conversation's agent
 * (useAgentPlan), named as the outside-matter stop names it (stops.ts). `names`: the app's matter names, by id, for the
 * notices that name other matters. */
export function noticeLine(
  kind: NoticeKind | undefined,
  w: NoticeWords,
  agent?: AgentName,
  names: MatterNameOf = NO_NAMES,
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
  if (typeof kind === "string") {
    if (kind === "context_not_restored")
      return w.th(
        "Clauseo doesn't remember the earlier questions and answers in this conversation. Include what it needs from them in your next question.",
      );
    if (kind === "auto_mode_unavailable")
      return w.th(
        "Claude's automatic mode isn't available for this account, so Claude asks before running commands.",
      );
    // another matter inside this one, and its entries couldn't be re-opened safely for Claude's sandbox (UC-382)
    if (kind === "shell_limited")
      return w.th(
        "Claude's commands can't open this matter's files right now; it can still read and edit them.",
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
  const refused = refusedMatterOf(kind, names);
  if (refused !== undefined)
    return refused.name === "" ? null : otherRefused(refused.name, w, agent);
  const loose =
    typeof kind === "object" && kind !== null && "matters_unfenced" in kind
      ? kind.matters_unfenced
      : undefined;
  if (loose !== undefined)
    return Array.isArray(loose.matters)
      ? unfenced(loose.matters, loose.more, w, names, agent)
      : null;
  const skipped =
    typeof kind === "object" && kind !== null && "skill_pack_not_loaded" in kind
      ? kind.skill_pack_not_loaded
      : undefined;
  if (skipped === undefined || typeof skipped.pack !== "string") return null;
  return packSkipped(skipped.pack, skipped.reason, w);
}
