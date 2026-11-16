// The sentences of the codes whose line names Claude, for the other agents (PLANNED in words-errors.ts' errorWords).
// Apart from words-errors.ts to keep it under the size limit; the Hindi is in words-errors.hi.ts with the rest.
import { BACKSLASH } from "./words-errors-backslash.ts";
import type { ErrorKey, WordedCode } from "./words-errors.ts";

/** The codes whose sentence names Claude, with their sentence for a thread on the ChatGPT plan, for an agent of no kind
 * by its name (`{name}`), and for one with no name at all. */
export const PLANNED: Partial<
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
  // provisional (U3-err-agent.guard_selftest_failed, Hindi later)
  "agent.guard_selftest_failed": {
    chatgpt:
      "Clauseo couldn't start its file check, so ChatGPT didn't start. Quit and reopen Clauseo, then try again.",
    named:
      "Clauseo couldn't start its file check, so {name} didn't start. Quit and reopen Clauseo, then try again.",
    nameless:
      "Clauseo couldn't start its file check, so the AI didn't start. Quit and reopen Clauseo, then try again.",
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
  // a folder whose path has a backslash, where the agent can't start (see ERROR_WORDS). Draft English (Rohan to
  // approve); the Hindi is provisional
  "matter.folder_backslash": BACKSLASH.finder.own,
  // the agent couldn't open the matter folder as it started (see ERROR_WORDS). Draft English (Rohan to approve); the
  // Hindi is provisional
  "agent.folder_inaccessible": {
    chatgpt:
      "ChatGPT couldn't open the matter's folder. Check that it's still there and hasn't been renamed, then Try again.",
    named:
      "{name} couldn't open the matter's folder. Check that it's still there and hasn't been renamed, then Try again.",
    nameless:
      "The AI couldn't open the matter's folder. Check that it's still there and hasn't been renamed, then Try again.",
  },
  // the agent couldn't get through to its service while the computer seemed online (offline truth; see ERROR_WORDS):
  // Try again, never Restart; provisional
  "agent.unreachable": {
    chatgpt:
      "Clauseo couldn't reach ChatGPT. The answer so far is kept. Try again. If this keeps happening, ask your firm's IT.",
    named:
      "Clauseo couldn't reach {name}. The answer so far is kept. Try again. If this keeps happening, ask your firm's IT.",
    nameless:
      "Clauseo couldn't reach the AI. The answer so far is kept. Try again. If this keeps happening, ask your firm's IT.",
  },
};
