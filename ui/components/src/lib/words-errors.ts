// The words for every error the host can return, chosen by its code and never its message (the message is for logs):
// one map from code to words. They stand on the working line after a refused send, and beside Undo after a refused undo.
// Each says what happened in plain words, and what stays kept where that is true. As in words.ts the English is the
// key, the exact string on screen. The Hindi is a specialist's draft, waiting for a Hindi-speaking lawyer: the formal
// आप, the danda, Western digits, English names in Latin script. Nothing here says मैं, पेंसिल, स्याही, चरण or कदम.
import type { Lang } from "./display.ts";
import { wordsOf } from "./words-core.ts";
import { th } from "./words-thread.ts";
import type { HI } from "./words-errors.hi.ts";
import type { VersionKey } from "./words-versions.ts";

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
  | "matter.folder_not_a_matter"
  | "matter.folder_permission"
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
  | "matter.folder_in_clauseo_data"
  | "matter.folder_inside_matter"
  | "matter.folder_holds_matter"
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
  | "agent.install_setup_failed"
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
  | "sandbox.blocked"
  | "blocklist.override_off"
  | "blocklist.notice_expired"
  | "blocklist.allow_not_saved"
  | "blocklist.revoke_not_saved"
  | "sandbox.cleaning_up"
  | "blackline.no_word_tool"
  | "blackline.not_word"
  | "blackline.failed"
  | "word.stetdoc_missing"
  | "word.revisions_stale"
  | "word.not_docx"
  | "word.edit_failed"
  | "agent.offline"
  | "prompt.too_long"
  | "mcp.name_empty"
  | "mcp.name_taken"
  | "mcp.name_reserved"
  | "mcp.name_invalid"
  | "mcp.command_invalid"
  | "mcp.command_not_found"
  | "mcp.url_invalid"
  | "mcp.header_name_invalid"
  | "mcp.env_name_invalid"
  | "mcp.secret_store_failed"
  | "mcp.server_unknown"
  | "mcp.save_declined"
  | "agent.service_error"
  | "agent.quiet"
  | "mcp.import_too_large"
  | "unknown";

/** The thread's own sentence for an answer cut short: its Hindi lives in words-thread.ts, once. */
const STOPPED = "The answer stopped. Your question and the answer so far are kept." as const;

/** The Word blackline's sentences, said in Versions over the sheet: their words live in words-versions.ts, once, and
 * the copy review registers them there (UC-114, UC-115). */
export const MARK_KEYS = [
  "Clauseo's Word tool is missing. Reinstall Clauseo to see the marked-up changes.",
  "The changes in this document couldn't be marked up.",
] as const satisfies readonly VersionKey[];
type MarkKey = (typeof MARK_KEYS)[number];
const isMark = (key: ErrorKey): key is MarkKey => (MARK_KEYS as readonly string[]).includes(key);

/** Every error sentence, in English. */
export type ErrorKey = keyof typeof HI | typeof STOPPED | MarkKey;

/** matter.folder_permission's button on the Mac, which opens macOS's Files and Folders privacy pane, and the line under
 * it (room/thread/privacy-settings.tsx). Settled by the orchestrator (2026-11-10); copy review pending. */
export const PRIVACY_KEYS = {
  button: "Open Privacy settings",
  line: "Turn on access for Clauseo, then ask again.",
} as const satisfies Record<string, ErrorKey>;

/** Codes still on the wire whose words are retired: they read as a code this version doesn't know ("unknown", as
 * internal reads). signin.endpoint_in_use sent the lawyer to a firm's address in Settings, Plans, which no longer has
 * one; only a hand-set address in testing reaches it (IT sweep). */
export type WordlessCode = "signin.endpoint_in_use";

/** A code that has its own sentence. */
export type WordedCode = Exclude<ErrorCode, WordlessCode>;

/** One sentence per code, in English; none for a WordlessCode. */
export const ERROR_WORDS: Record<WordedCode, ErrorKey> = {
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
  "agent.auth_required": "Sign in with Claude first. Your question is kept.",
  "agent.unsupported_version": "This version of Claude Code doesn't work with Clauseo.",
  "store.disk_full": "The computer's disk is full. Free some space, then try again.",
  "store.corrupt": "Clauseo couldn't read back its saved data.",
  "store.io": "Clauseo couldn't read or write its data. Try again.",
  "store.locked": "Clauseo is already open in another window.",
  "store.newer_version": "This data comes from a newer version of Clauseo.",
  "matter.checkpoint_failed":
    "Clauseo couldn't save a version of this matter's papers, so this change can't be undone.",
  "matter.folder_missing":
    "This matter's folder can't be reached. It may have moved, or its drive or network isn't connected.",
  "matter.folder_not_a_matter":
    "This folder doesn't hold this matter's Clauseo file. Locate the matter's folder to carry on.",
  // settled (orchestrator, 2026-11-10); the app's Open Privacy settings button and its line are the room's
  "matter.folder_permission": "Clauseo needs permission to open this folder.",
  "matter.folder_refused": "A whole drive, your home folder or a system folder can't be a matter.",
  "matter.codex_folder":
    "This matter's folder holds a .codex folder. Rename or remove it to use ChatGPT here.",
  "matter.file_locked": "A paper is open in another program. Close it, then try again.",
  "matter.turn_running": "Wait for the answer to finish first.",
  // folder PR 2 (approved 2026-11-11): each folder refusal says its reason; a folder nested with a matter, here where
  // no name is at hand, and the rare folder_is_matter take the line of a folder Clauseo can't make a matter of
  "matter.folder_is_drive": "This is the top of a drive. Choose a folder on it.",
  "matter.folder_is_home": "This is your home folder. Choose a folder inside it.",
  "matter.folder_is_system": "This is a system folder. Choose another folder.",
  "matter.folder_unreadable": "Clauseo can't read that folder.",
  "matter.folder_is_matter": "Clauseo can't make a matter of this folder. Choose another folder.",
  "matter.folder_in_matters_root":
    "That folder is where Clauseo keeps matters. Choose the folder for one matter.",
  "matter.folder_in_clauseo_data":
    "This folder is part of Clauseo's own data. Choose another folder.",
  "matter.folder_inside_matter":
    "Clauseo can't make a matter of this folder. Choose another folder.",
  "matter.folder_holds_matter":
    "Clauseo can't make a matter of this folder. Choose another folder.",
  "matter.folder_network":
    "ChatGPT can't work on a network drive. Move the matter to this computer.",
  "matter.folder_no_permissions":
    "ChatGPT can't work on a drive without file permissions. Move the matter to this computer.",
  // also a turn refused in an open matter, so not folder PR 2's choose-another line (that is matter-form-check.ts's)
  "matter.folder_too_deep":
    "This matter's folder is too deep inside other folders for Clauseo to work in it on Windows. Move it nearer the top of its drive.",
  "transport.unauthorized": "That wasn't allowed, so nothing changed.",
  "transport.too_large": "That is too large to send.",
  "settings.locked": "Your firm manages that setting, so it can't be changed.",
  "settings.refused": "Clauseo couldn't save that setting. Your settings are as they were.",
  "settings.key_stored":
    "A key is already kept on this computer. To change it, open Settings, then Plans.",
  "signin.login_shared":
    "Clauseo is using the ChatGPT login already on this computer. To change it, choose 'Stop using it here', then sign in.",
  "signin.firm_managed":
    "Questions go through your firm's own account, so there's nothing to sign in to here.",
  "signin.key_in_use":
    "Clauseo is using your API key. To sign in with your plan instead, open Settings, then Plans, and remove the key.",
  "agent.install_offline": "You're offline. Setting up waits for the connection.",
  "agent.install_blocked": "Your network is blocking the download.",
  "agent.install_disk_full": "There isn't enough free space on this computer for the download.",
  "agent.install_failed": "Setting up didn't finish. Try again.",
  // provisional (install resume PR): copy review and Hindi review are the app's
  "agent.install_setup_failed":
    "Claude was downloaded, but setting it up didn't finish. Try again.",
  "signin.cancelled": "Sign-in was cancelled.",
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
  // provisional (app track, Rohan's batch review): the sandbox's four (decision record 0051), in
  // design B's "Safe workspace" words
  "sandbox.setup_refused":
    "Windows didn't get permission for the change Clauseo asked it to approve, so nothing changed. Try again, and choose Yes when Windows asks.",
  "sandbox.setup_failed": "The Safe workspace couldn't be set up on this computer. Try again.",
  "sandbox.setup_required":
    "Clauseo can't answer here until the Safe workspace is set up on this computer. Your question is kept.",
  "sandbox.required":
    "Your firm requires the Safe workspace, and this computer can't provide it, so Clauseo can't answer here.",
  // provisional (app track, Rohan's batch review): in design B's "Safe workspace" words
  "sandbox.blocked":
    "Clauseo can't use the Safe workspace on this computer: a Windows setting or security software is blocking it, so nothing was started. Settings › Safe workspace says what to do.",
  // provisional (stage E, Rohan's batch review): "Allow this site" refused while the firm's policy forbids overriding
  // the unsafe-site list (decision record 0053)
  "blocklist.override_off":
    "Your firm has turned off allowing sites on Clauseo's unsafe-site list, so this site stays blocked.",
  // provisional (stage E, Rohan's batch review): "Allow this site" from a blocked-site notice the host no longer
  // keeps (decision record 0053)
  "blocklist.notice_expired":
    "This notice has expired. Open the site again, then allow it from the new notice.",
  // provisional (stage E, Rohan's batch review): "Allow this site" that lasts until Clauseo
  // closes, because the host couldn't save it (decision record 0053)
  "blocklist.allow_not_saved":
    "This site is allowed until Clauseo closes. Clauseo couldn't save the change, so it will be blocked again next time Clauseo opens.",
  // provisional (stage E, Rohan's batch review): "Remove" of an allow that lasts until
  // Clauseo closes, because the host couldn't save that (decision record 0053)
  "blocklist.revoke_not_saved":
    "This site is blocked again until Clauseo closes. Clauseo couldn't save the change, so it may be allowed again next time Clauseo opens.",
  // provisional (sandbox track, for the app thread to replace): a launch refused while an earlier
  // session's clean-up is still running (decision record 0049); retryable
  "sandbox.cleaning_up":
    "Clauseo is still tidying up the Safe workspace after it last closed. Try again in a moment. Your question is kept.",
  // provisional (UC-114, UC-115): the Word blackline's refusals, as Versions says them over the sheet
  "blackline.no_word_tool":
    "Clauseo's Word tool is missing. Reinstall Clauseo to see the marked-up changes.",
  "blackline.not_word": "The changes in this document couldn't be marked up.",
  "blackline.failed": "The changes in this document couldn't be marked up.",
  // provisional (English settled by the orchestrator): a Word file's tracked changes, accepted or rejected from its
  // tile on the wall (room/word/changes-pane.tsx); edit_failed's line stands beside Try again
  "word.stetdoc_missing":
    "Clauseo's Word tool is missing. Reinstall Clauseo to accept or reject changes.",
  "word.revisions_stale":
    "This file changed after the list was made, so nothing was accepted or rejected. The list now shows its current changes.",
  "word.not_docx":
    "Clauseo can't read the tracked changes in this file. It may be damaged, or it may not be a Word document.",
  "word.edit_failed": "Clauseo couldn't save your decision. The file is as it was.",
  // the connection dropped mid-answer (D-050); provisional, with Try again under the answer
  "agent.offline":
    "You're offline, so the answer stopped. The answer so far is kept. Try again when you're back online.",
  // a question over the host's cap on a prompt's text (MAX_PROMPT_TEXT_BYTES): refused by the host, or by the composer
  // before it sends; the draft stays in the field. Settled by the orchestrator (2026-11-11)
  "prompt.too_long":
    "This question is too long to send. Save the text as a file in the matter's folder, then ask about it. Your words are kept.",
  // the lawyer's MCP servers (Settings › Tools): the Tools form says each in its own words; anywhere else,
  // the settings window's failure line
  "mcp.name_empty": "Clauseo couldn't save that setting. Your settings are as they were.",
  "mcp.name_taken": "Clauseo couldn't save that setting. Your settings are as they were.",
  "mcp.name_reserved": "Clauseo couldn't save that setting. Your settings are as they were.",
  "mcp.name_invalid": "Clauseo couldn't save that setting. Your settings are as they were.",
  "mcp.command_invalid": "Clauseo couldn't save that setting. Your settings are as they were.",
  "mcp.command_not_found": "Clauseo couldn't save that setting. Your settings are as they were.",
  "mcp.url_invalid": "Clauseo couldn't save that setting. Your settings are as they were.",
  "mcp.header_name_invalid": "Clauseo couldn't save that setting. Your settings are as they were.",
  "mcp.env_name_invalid": "Clauseo couldn't save that setting. Your settings are as they were.",
  "mcp.secret_store_failed": "Clauseo couldn't save that setting. Your settings are as they were.",
  "mcp.server_unknown": "Clauseo couldn't save that setting. Your settings are as they were.",
  // Cancel in the native confirm before a server is added or what it starts changes
  "mcp.save_declined": "You chose Cancel, so nothing was saved.",
  // the agent's service had a problem on its side and the agent gave up on the turn (#1030): Try again under the
  // answer, never Restart. ChatGPT's sentence approved (orchestrator, 2026-11-12); Claude's derived from it
  "agent.service_error": "Claude had a problem on its side. Try again.",
  // the agent sent nothing for 5 minutes while answering, with no tool running and no question waiting, so the host
  // ended the turn (D-050's backstop): Try again under the answer so far, never Restart. English approved (orchestrator,
  // 2026-11-13); the Hindi is provisional
  "agent.quiet": "Claude stopped answering. Try again.",
  // an import whose servers are too long to show whole in one confirm: nothing was added
  "mcp.import_too_large":
    "There are too many tools to import at once, so nothing was imported. Add them one at a time.",
  // a code from a newer Clauseo: as internal reads, since this version can say nothing truer
  unknown: "Something went wrong inside Clauseo.",
};

/** Codes the app gives itself, never the host, so outside ErrorCode: an answer's link, or Versions' Open now, to a
 * paper the matter's folder no longer holds (room/open.ts). */
export const APP_ERROR_WORDS = {
  "paper.not_in_folder": "This paper is no longer in the matter's folder.",
} as const satisfies Record<string, ErrorKey>;

const own = wordsOf("errors");
/** Versions' words, read from the shared list rather than words-versions.ts, which stays out of the room's first
 * load (first-load.test.ts). */
const versions = wordsOf("versions");

/** An error sentence in the language. */
export function errorSentence(lang: Lang, key: ErrorKey): string {
  if (key === STOPPED) return th(lang, key);
  return isMark(key) ? versions(lang, key) : own(lang, key);
}

/** The codes whose sentence names Claude, with their sentence for a thread on the ChatGPT plan, for an agent of no kind
 * by its name (`{name}`), and for one with no name at all. */
const PLANNED: Partial<
  Record<WordedCode, { chatgpt: ErrorKey; named: ErrorKey; nameless: ErrorKey }>
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
    chatgpt: "Sign in with ChatGPT in Codex first. Your question is kept.",
    named: "Sign in with {name} first. Your question is kept.",
    nameless: "Sign in first. Your question is kept.",
  },
  "agent.unsupported_version": {
    chatgpt: "This version of Codex doesn't work with Clauseo.",
    named: "This version of {name} doesn't work with Clauseo.",
    nameless: "This version of the AI doesn't work with Clauseo.",
  },
  // provisional (install resume PR), as ERROR_WORDS' sentence
  "agent.install_setup_failed": {
    chatgpt: "Codex was downloaded, but setting it up didn't finish. Try again.",
    named: "{name} was downloaded, but setting it up didn't finish. Try again.",
    nameless: "The AI was downloaded, but setting it up didn't finish. Try again.",
  },
  "agent.guard_missing": {
    chatgpt: "Clauseo can't start ChatGPT safely here. Reinstall Clauseo.",
    named: "Clauseo can't start {name} safely here. Reinstall Clauseo.",
    nameless: "Clauseo can't start the AI safely here. Reinstall Clauseo.",
  },
  // ChatGPT's approved (orchestrator, 2026-11-12); {name}'s and the AI's derived from it (provisional)
  "agent.service_error": {
    chatgpt: "ChatGPT had a problem on its side. Try again.",
    named: "{name} had a problem on its side. Try again.",
    nameless: "The AI had a problem on its side. Try again.",
  },
  // the agent sent nothing for 5 minutes (D-050's backstop; see ERROR_WORDS): Try again, never Restart. English
  // approved (orchestrator, 2026-11-13); the Hindi is provisional
  "agent.quiet": {
    chatgpt: "ChatGPT stopped answering. Try again.",
    named: "{name} stopped answering. Try again.",
    nameless: "The AI stopped answering. Try again.",
  },
};

/** The codes PLANNED gives a sentence per agent. */
export const PLANNED_CODES = Object.keys(PLANNED) as WordedCode[];

/** The thread's agent as the words name it: its kind, which picks the plan's line (Claude's or ChatGPT's), and its
 * name, for an agent of no kind (the agent's own name from its header when the host has one, else its settings id);
 * null with no name at all; and whether the plan runs on an API key. The whole is undefined where the caller doesn't know the thread. The kind is the app's
 * AgentKind, copied as ErrorCode is. */
export type AgentName =
  | {
      kind: "claude" | "codex" | undefined;
      name: string | null;
      /** The plan runs on the lawyer's API key: its refusal (agent.auth_required) is the key's, never a sign-in. */
      key?: boolean | undefined;
    }
  | undefined;

/** agent.auth_required on a plan that runs on an API key: the provider refused the key, so the line names the key. */
const KEY_REFUSED = "{name} didn't accept the API key. Your question is kept." satisfies ErrorKey;

/** A code's sentence for the thread's agent: a codex agent picks PLANNED's ChatGPT sentence; an agent of no kind its
 * `{name}` sentence with its name, or its nameless one with no name (or an empty one); a claude agent, an agent not
 * known (undefined), or a code PLANNED doesn't hold, the code's own sentence (ERROR_WORDS, Claude's). */
export function plannedKey(code: WordedCode, agent?: AgentName): { key: ErrorKey; name?: string } {
  // a key in use: the key wasn't accepted, by the plan's name (the agent's own for one of no kind)
  if (code === "agent.auth_required" && agent?.key === true) {
    const name =
      agent.kind === "codex"
        ? "ChatGPT"
        : agent.kind === "claude"
          ? "Claude"
          : agent.name?.trim() || null;
    if (name !== null) return { key: KEY_REFUSED, name };
  }
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
  if (Object.hasOwn(APP_ERROR_WORDS, code))
    return own(lang, APP_ERROR_WORDS[code as keyof typeof APP_ERROR_WORDS]);
  const known = Object.hasOwn(ERROR_WORDS, code) ? (code as WordedCode) : "unknown";
  const { key, name } = plannedKey(known, agent);
  return name === undefined || key === STOPPED || isMark(key)
    ? errorSentence(lang, key)
    : own(lang, key, { name });
}
