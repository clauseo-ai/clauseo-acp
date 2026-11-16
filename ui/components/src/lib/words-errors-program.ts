// The lines of agent.program_missing and agent.program_blocked (words-errors.ts): the agent's program is not on this
// computer, or the computer refused to run it, so the agent didn't start. agent.program_blocked's Windows line says
// where Windows Security lets it run; the Mac's says it in general. Apart from words-errors.ts to keep it under the size
// limit; their Hindi (none yet) is in words-errors.hi.ts with the rest. Provisional (the app's words, via the
// orchestrator).
import type { ByAgent } from "./words-errors-backslash.ts";

/** agent.program_missing's lines by the thread's agent, the same on every computer (U3-err-agent.program_missing,
 * .chatgpt, .named, .nameless). */
export const PROGRAM_MISSING: ByAgent = {
  claude:
    "A program Claude needs is missing from this computer, so Claude didn't start. Quit Clauseo and open it again, then choose Install when Clauseo offers it. Your question is kept.",
  chatgpt:
    "A program ChatGPT needs is missing from this computer, so ChatGPT didn't start. Quit Clauseo and open it again, then choose Install when Clauseo offers it. Your question is kept.",
  named:
    "A program {name} needs is missing from this computer, so {name} didn't start. Quit Clauseo and open it again, then choose Install when Clauseo offers it. Your question is kept.",
  nameless:
    "A program the AI needs is missing from this computer, so the AI didn't start. Quit Clauseo and open it again, then choose Install when Clauseo offers it. Your question is kept.",
};

/** agent.program_blocked's lines off the Mac (U3-err-agent.program_blocked, .chatgpt, .named, .nameless). */
export const PROGRAM_BLOCKED: ByAgent = {
  claude:
    "This computer stopped a program Claude needs from starting, so Claude didn't start. If your antivirus blocked it, open Windows Security, choose Protection history, and allow it, then try again. Otherwise, whoever looks after this computer can allow it. Your question is kept.",
  chatgpt:
    "This computer stopped a program ChatGPT needs from starting, so ChatGPT didn't start. If your antivirus blocked it, open Windows Security, choose Protection history, and allow it, then try again. Otherwise, whoever looks after this computer can allow it. Your question is kept.",
  named:
    "This computer stopped a program {name} needs from starting, so {name} didn't start. If your antivirus blocked it, open Windows Security, choose Protection history, and allow it, then try again. Otherwise, whoever looks after this computer can allow it. Your question is kept.",
  nameless:
    "This computer stopped a program the AI needs from starting, so the AI didn't start. If your antivirus blocked it, open Windows Security, choose Protection history, and allow it, then try again. Otherwise, whoever looks after this computer can allow it. Your question is kept.",
};

/** The same on the Mac (each id's .mac). */
export const PROGRAM_BLOCKED_MAC: ByAgent = {
  claude:
    "This computer stopped a program Claude needs from starting, so Claude didn't start. If your antivirus blocked it, allow it there, then try again. Otherwise, whoever looks after this computer can allow it. Your question is kept.",
  chatgpt:
    "This computer stopped a program ChatGPT needs from starting, so ChatGPT didn't start. If your antivirus blocked it, allow it there, then try again. Otherwise, whoever looks after this computer can allow it. Your question is kept.",
  named:
    "This computer stopped a program {name} needs from starting, so {name} didn't start. If your antivirus blocked it, allow it there, then try again. Otherwise, whoever looks after this computer can allow it. Your question is kept.",
  nameless:
    "This computer stopped a program the AI needs from starting, so the AI didn't start. If your antivirus blocked it, allow it there, then try again. Otherwise, whoever looks after this computer can allow it. Your question is kept.",
};

/** The Mac's lines by code. */
export const PROGRAM_MAC: Readonly<Record<string, ByAgent>> = {
  "agent.program_blocked": PROGRAM_BLOCKED_MAC,
};
