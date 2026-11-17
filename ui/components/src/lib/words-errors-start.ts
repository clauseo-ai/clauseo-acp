// agent.start_unreachable's lines (words-errors.ts): a start failed on its connection while the computer is online and
// Clauseo couldn't reach the agent's service (a slow connection, a firm's network). The same on every computer. Apart
// from words-errors.ts to keep it under the size limit; their Hindi (none yet) is in words-errors.hi.ts with the rest.
// Provisional (the app's words, via the orchestrator).
import type { ByAgent } from "./words-errors-backslash.ts";

/** agent.start_unreachable's lines by the thread's agent (U3-err-agent.start_unreachable, .chatgpt, .named,
 * .nameless). */
export const START_UNREACHABLE: ByAgent = {
  claude:
    "Claude didn't start, because Clauseo couldn't reach Claude from this computer, though it's online. A slow connection, or your firm's network blocking it, can cause this. Your question is kept. Try again in a moment. If it keeps happening, show this to whoever looks after this computer.",
  chatgpt:
    "ChatGPT didn't start, because Clauseo couldn't reach ChatGPT from this computer, though it's online. A slow connection, or your firm's network blocking it, can cause this. Your question is kept. Try again in a moment. If it keeps happening, show this to whoever looks after this computer.",
  named:
    "{name} didn't start, because Clauseo couldn't reach {name} from this computer, though it's online. A slow connection, or your firm's network blocking it, can cause this. Your question is kept. Try again in a moment. If it keeps happening, show this to whoever looks after this computer.",
  nameless:
    "The AI didn't start, because Clauseo couldn't reach it from this computer, though it's online. A slow connection, or your firm's network blocking it, can cause this. Your question is kept. Try again in a moment. If it keeps happening, show this to whoever looks after this computer.",
};
