// The lines of agent.tool_missing (words-errors.ts): a file Clauseo keeps for the agent isn't as Clauseo left it, so the
// agent wasn't started. Windows' line says where Windows Security lets the file back; the Mac's and Linux's say it in
// general. Apart from words-errors.ts to keep it under the size limit; their Hindi (none yet) is in
// words-errors.hi.ts with the rest. Provisional (via the orchestrator).
import type { ByAgent } from "./words-errors-backslash.ts";

/** agent.tool_missing's lines by the thread's agent on Windows, and for a caller that doesn't know the computer
 * (U3-err-agent.tool_missing, .chatgpt, .named, .nameless). */
export const TOOL_MISSING: ByAgent = {
  claude:
    "Claude can't answer, because a file Clauseo keeps for it isn't as Clauseo left it. Quit Clauseo and open it again, and Clauseo puts it right. If Windows Security quarantined it, do this first: open Windows Security, choose Virus & threat protection, then Protection history, and restore or allow the item in Clauseo's folder. If it keeps happening, show this to whoever looks after this computer.",
  chatgpt:
    "ChatGPT can't answer, because a file Clauseo keeps for it isn't as Clauseo left it. Quit Clauseo and open it again, and Clauseo puts it right. If Windows Security quarantined it, do this first: open Windows Security, choose Virus & threat protection, then Protection history, and restore or allow the item in Clauseo's folder. If it keeps happening, show this to whoever looks after this computer.",
  named:
    "{name} can't answer, because a file Clauseo keeps for it isn't as Clauseo left it. Quit Clauseo and open it again, and Clauseo puts it right. If Windows Security quarantined it, do this first: open Windows Security, choose Virus & threat protection, then Protection history, and restore or allow the item in Clauseo's folder. If it keeps happening, show this to whoever looks after this computer.",
  nameless:
    "The AI can't answer, because a file Clauseo keeps for it isn't as Clauseo left it. Quit Clauseo and open it again, and Clauseo puts it right. If Windows Security quarantined it, do this first: open Windows Security, choose Virus & threat protection, then Protection history, and restore or allow the item in Clauseo's folder. If it keeps happening, show this to whoever looks after this computer.",
};

/** The same on the Mac and on Linux (each id's .mac). */
export const TOOL_MISSING_MAC: ByAgent = {
  claude:
    "Claude can't answer, because a file Clauseo keeps for it isn't as Clauseo left it. Quit Clauseo and open it again, and Clauseo puts it right. If your antivirus quarantined it, restore or allow it there first. If it keeps happening, show this to whoever looks after this computer.",
  chatgpt:
    "ChatGPT can't answer, because a file Clauseo keeps for it isn't as Clauseo left it. Quit Clauseo and open it again, and Clauseo puts it right. If your antivirus quarantined it, restore or allow it there first. If it keeps happening, show this to whoever looks after this computer.",
  named:
    "{name} can't answer, because a file Clauseo keeps for it isn't as Clauseo left it. Quit Clauseo and open it again, and Clauseo puts it right. If your antivirus quarantined it, restore or allow it there first. If it keeps happening, show this to whoever looks after this computer.",
  nameless:
    "The AI can't answer, because a file Clauseo keeps for it isn't as Clauseo left it. Quit Clauseo and open it again, and Clauseo puts it right. If your antivirus quarantined it, restore or allow it there first. If it keeps happening, show this to whoever looks after this computer.",
};

/** The Mac's and Linux's lines by code. */
export const TOOL_MAC: Readonly<Record<string, ByAgent>> = {
  "agent.tool_missing": TOOL_MISSING_MAC,
};
