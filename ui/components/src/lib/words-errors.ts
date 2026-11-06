// The words for every error the host can return, chosen by its code and never its message (the message is for logs):
// one map from code to words. They stand on the working line after a refused send, and beside Undo after a refused undo.
// Each says what happened in plain words, and what stays kept where that is true. As in words.ts the English is the
// key, the exact string on screen. The Hindi is a specialist's draft, waiting for a Hindi-speaking lawyer: the formal
// आप, the danda, Western digits, English names in Latin script. Nothing here says मैं, पेंसिल, स्याही, चरण or कदम.
import type { Lang } from "./display.ts";
import { wordsOf } from "./words-core.ts";
import { th } from "./words-thread.ts";
import type { HI } from "./words-errors.hi.ts";

/** Why a host call failed: the protocol's `ErrorCode` (ui/app/src/transport/bindings.ts), copied, since the parts
 * can't import the app. ui/app/src/room/thread/error-codes.test.ts fails when the two differ. */
export type ErrorCode =
  | "internal"
  | "command.invalid"
  | "command.conflict"
  | "command.unknown_matter"
  | "command.unknown_thread"
  | "thread.turn_running"
  | "thread.interrupted"
  | "thread.turn_not_queued"
  | "thread.queue_full"
  | "permission.not_pending"
  | "permission.unknown_option"
  | "elicitation.not_pending"
  | "elicitation.invalid_values"
  | "agent.launch_failed"
  | "agent.exited"
  | "agent.protocol_error"
  | "agent.auth_required"
  | "agent.unsupported_version"
  | "store.disk_full"
  | "store.corrupt"
  | "store.io"
  | "store.locked"
  | "store.newer_version"
  | "matter.checkpoint_failed"
  | "matter.folder_missing"
  | "matter.folder_refused"
  | "matter.codex_folder"
  | "matter.file_locked"
  | "matter.turn_running"
  | "matter.folder_is_drive"
  | "matter.folder_is_home"
  | "matter.folder_is_system"
  | "matter.folder_unreadable"
  | "matter.folder_is_matter"
  | "matter.folder_in_matters_root"
  | "matter.folder_network"
  | "matter.folder_no_permissions"
  | "matter.folder_too_deep"
  | "transport.unauthorized"
  | "transport.too_large"
  | "settings.locked"
  | "settings.refused"
  | "settings.key_stored"
  | "signin.login_shared"
  | "signin.firm_managed"
  | "signin.key_in_use"
  | "signin.endpoint_in_use"
  | "agent.install_offline"
  | "agent.install_blocked"
  | "agent.install_disk_full"
  | "agent.install_failed"
  | "signin.cancelled"
  | "signin.link_expired"
  | "signin.no_plan"
  | "signin.device_code_off"
  | "signin.port_busy"
  | "signin.failed"
  | "signin.policy_unreadable"
  | "agent.guard_missing"
  | "update.failed"
  | "signin.agents_still_running"
  | "agent.keys_loading"
  | "agent.key_unreadable"
  | "matter.document_path_too_long"
  | "matter.protected_link"
  | "sandbox.setup_refused"
  | "sandbox.setup_failed"
  | "sandbox.setup_required"
  | "sandbox.required"
  | "signin.reuse_other_volume"
  | "unknown";

/** The thread's own sentence for an answer cut short: its Hindi lives in words-thread.ts, once. */
const STOPPED = "The answer stopped. Your question and the answer so far are kept." as const;

/** Every error sentence, in English. */
export type ErrorKey = keyof typeof HI | typeof STOPPED;

/** One sentence per code, in English. */
export const ERROR_WORDS: Record<ErrorCode, ErrorKey> = {
  internal: "Something went wrong inside Clauseo.",
  "command.invalid": "Clauseo couldn't take that request as it was.",
  "command.conflict": "That request clashes with one already made, so nothing changed.",
  "command.unknown_matter": "This matter is no longer in Clauseo.",
  "command.unknown_thread": "This conversation is no longer in Clauseo.",
  "thread.turn_running": "Wait for the answer to finish, or stop it.",
  "thread.interrupted": STOPPED,
  "thread.turn_not_queued": "The answer to that question has already started or finished.",
  "thread.queue_full": "Too many questions are waiting in this conversation.",
  "permission.not_pending": "That request was already answered, or it has expired.",
  "permission.unknown_option": "That answer wasn't one of the choices offered.",
  "elicitation.not_pending": "That question was already answered, or it has expired.",
  "elicitation.invalid_values": "Those answers don't fit the question.",
  "agent.launch_failed": "Claude didn't start. Your question is kept.",
  "agent.exited": STOPPED,
  "agent.protocol_error": "Claude sent something Clauseo can't read.",
  "agent.auth_required": "Sign in to Claude first. Your question is kept.",
  "agent.unsupported_version": "This version of Claude Code doesn't work with Clauseo.",
  "store.disk_full": "The computer's disk is full. Free some space, then try again.",
  "store.corrupt": "Clauseo couldn't read back its saved data.",
  "store.io": "Clauseo couldn't read or write its data. Try again.",
  "store.locked": "Clauseo is already open in another window.",
  "store.newer_version": "This data comes from a newer version of Clauseo.",
  "matter.checkpoint_failed":
    "Clauseo couldn't save a version of this matter's papers, so this change can't be undone.",
  "matter.folder_missing":
    "This matter's folder can't be found. It may have moved, or its drive isn't connected.",
  "matter.folder_refused": "A whole drive, your home folder or a system folder can't be a matter.",
  "matter.codex_folder":
    "This matter's folder holds a .codex folder. Rename or remove it to use Codex here.",
  "matter.file_locked": "A paper is open in another program. Close it, then try again.",
  "matter.turn_running": "Wait for the answer to finish first.",
  "matter.folder_is_drive": "A whole drive can't be a matter.",
  "matter.folder_is_home": "Your home folder can't be a matter.",
  "matter.folder_is_system": "A system folder can't be a matter.",
  "matter.folder_unreadable": "Clauseo can't read that folder.",
  "matter.folder_is_matter": "That folder is already a matter's, or inside one.",
  "matter.folder_in_matters_root": "That folder holds Clauseo's own data.",
  "matter.folder_network": "Codex can't work on a network drive. Move the matter to this computer.",
  "matter.folder_no_permissions":
    "Codex can't work on a drive without file permissions. Move the matter to this computer.",
  "matter.folder_too_deep":
    "This matter's folder is too deep inside other folders for Clauseo to work in it on Windows. Move it nearer the top of its drive.",
  "transport.unauthorized": "That wasn't allowed, so nothing changed.",
  "transport.too_large": "That is too large to send.",
  "settings.locked": "Your organisation manages that setting, so it can't be changed.",
  "settings.refused": "Clauseo couldn't save that setting. Your settings are as they were.",
  "settings.key_stored":
    "A key is already kept on this computer. To change it, open Settings, then Plans.",
  "signin.login_shared":
    "Clauseo is using the ChatGPT login already on this computer. To change it, choose 'Stop using it here', then sign in.",
  "signin.firm_managed":
    "Questions go through your firm's own account, so there's nothing to sign in to here.",
  "signin.key_in_use":
    "Clauseo is using your API key. To sign in with your plan instead, open Settings, then Plans, and remove the key.",
  "signin.endpoint_in_use":
    "Clauseo is using your firm's address. To sign in with your plan instead, open Settings, then Plans, and remove the address.",
  "agent.install_offline": "You're offline. Setting up waits for the connection.",
  "agent.install_blocked": "Your network is blocking the download.",
  "agent.install_disk_full": "There isn't enough free space on this computer for the download.",
  "agent.install_failed": "Setting up didn't finish. Try again.",
  "signin.cancelled": "Sign-in was cancelled in the browser.",
  "signin.link_expired": "The sign-in link expired. Try again.",
  "signin.no_plan": "This account has no plan that works with Clauseo.",
  "signin.device_code_off": "Sign-in with a code is turned off for this ChatGPT account.",
  "signin.port_busy": "Another program on this computer is using ChatGPT's sign-in.",
  "signin.failed": "Sign-in didn't finish. Try again.",
  "signin.policy_unreadable":
    "Clauseo can't read your firm's sign-in settings, so it won't sign in until they can be read.",
  "agent.guard_missing": "Clauseo can't start Claude safely here. Reinstall Clauseo.",
  "update.failed": "The update didn't install. Try again.",
  "signin.agents_still_running": "Clauseo is still finishing. Try again in a moment.",
  "agent.keys_loading": "Clauseo is still starting. Try again in a moment.",
  "agent.key_unreadable":
    "Clauseo couldn't check for a saved API key in time. If this computer asks for permission, allow it, then try again.",
  "matter.document_path_too_long":
    "This file's path is too long for Word. Move the matter's folder nearer the top of the drive, or shorten its folder names.",
  // provisional: the item's path joins it once PR6 carries it as a field
  "matter.protected_link":
    "A file in this matter's folder is a shortcut. Clauseo can't protect a shortcut, so it won't answer in this matter until it's gone. Move or delete it, then ask again.",
  // provisional (app track, Rohan's batch review): the sandbox's four (decision record 0051)
  "sandbox.setup_refused":
    "Protection wasn't set up, because Windows didn't get permission. Try again, and choose Yes when Windows asks.",
  "sandbox.setup_failed":
    "Protection couldn't be set up on this computer. Try again. If it keeps failing, ask whoever looks after this computer to look into it.",
  "sandbox.setup_required":
    "Clauseo needs to set up protection on this computer once before it can answer here. Set it up, then ask again. Your question is kept.",
  "sandbox.required":
    "Your firm requires protection, and this computer can't provide it, so Clauseo can't answer here. Ask whoever looks after this computer.",
  "signin.reuse_other_volume":
    "Clauseo can't reuse your Codex login because its data folder is on a different disk. Move Clauseo's data folder to the same disk, or sign in to Codex in Clauseo.",
  // a code from a newer Clauseo: as internal reads, since this version can say nothing truer
  unknown: "Something went wrong inside Clauseo.",
};

const own = wordsOf("errors");

/** An error sentence in the language. */
export function errorSentence(lang: Lang, key: ErrorKey): string {
  return key === STOPPED ? th(lang, key) : own(lang, key);
}

/** The codes whose sentence names Claude, with their sentence for a thread on the ChatGPT plan, for an agent of no kind
 * by its name (`{name}`), and for one with no name at all. */
const PLANNED: Partial<
  Record<ErrorCode, { chatgpt: ErrorKey; named: ErrorKey; nameless: ErrorKey }>
> = {
  "agent.launch_failed": {
    chatgpt: "ChatGPT didn't start. Your question is kept.",
    named: "{name} didn't start. Your question is kept.",
    nameless: "The AI didn't start. Your question is kept.",
  },
  "agent.protocol_error": {
    chatgpt: "ChatGPT sent something Clauseo can't read.",
    named: "{name} sent something Clauseo can't read.",
    nameless: "The AI sent something Clauseo can't read.",
  },
  "agent.auth_required": {
    chatgpt: "Sign in to ChatGPT in Codex first. Your question is kept.",
    named: "Sign in to {name} first. Your question is kept.",
    nameless: "Sign in to the AI first. Your question is kept.",
  },
  "agent.unsupported_version": {
    chatgpt: "This version of Codex doesn't work with Clauseo.",
    named: "This version of {name} doesn't work with Clauseo.",
    nameless: "This version of the AI doesn't work with Clauseo.",
  },
  "agent.guard_missing": {
    chatgpt: "Clauseo can't start ChatGPT safely here. Reinstall Clauseo.",
    named: "Clauseo can't start {name} safely here. Reinstall Clauseo.",
    nameless: "Clauseo can't start the AI safely here. Reinstall Clauseo.",
  },
};

/** The codes PLANNED gives a sentence per agent. */
export const PLANNED_CODES = Object.keys(PLANNED) as ErrorCode[];

/** The thread's agent as the words name it: its kind, which picks the plan's line (Claude's or ChatGPT's), and its
 * name, for an agent of no kind (the agent's own name from its header when the host has one, else its settings id);
 * null with no name at all. The whole is undefined where the caller doesn't know the thread. The kind is the app's
 * AgentKind, copied as ErrorCode is. */
export type AgentName = { kind: "claude" | "codex" | undefined; name: string | null } | undefined;

/** A code's sentence for the thread's agent: a codex agent picks PLANNED's ChatGPT sentence; an agent of no kind its
 * `{name}` sentence with its name, or its nameless one with no name (or an empty one); a claude agent, an agent not
 * known (undefined), or a code PLANNED doesn't hold, the code's own sentence (ERROR_WORDS, Claude's). */
export function plannedKey(code: ErrorCode, agent?: AgentName): { key: ErrorKey; name?: string } {
  const planned = PLANNED[code];
  if (planned === undefined || agent === undefined || agent.kind === "claude")
    return { key: ERROR_WORDS[code] };
  if (agent.kind === "codex") return { key: planned.chatgpt };
  const name = agent.name?.trim() ?? "";
  return name === "" ? { key: planned.nameless } : { key: planned.named, name };
}

/** A code's sentence in the language; a code this version doesn't know reads as "unknown", which reads as "internal".
 * `agent` is the thread's agent (AgentName), for the codes whose sentence names it (plannedKey); a caller that
 * doesn't know the thread leaves it out. */
export function errorWords(lang: Lang, code: string, agent?: AgentName): string {
  const known = Object.hasOwn(ERROR_WORDS, code) ? (code as ErrorCode) : "unknown";
  const { key, name } = plannedKey(known, agent);
  return name === undefined || key === STOPPED
    ? errorSentence(lang, key)
    : own(lang, key, { name });
}
