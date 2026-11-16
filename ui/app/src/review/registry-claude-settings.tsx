// The copy review entries for a Claude launch the host refused, or a Claude it stopped, over Claude's own settings
// (UC-474 to UC-476, #launch-never-weaker): the firm's managed settings turn hooks off (agent.firm_hooks_off), a policy
// any program can write changed its security settings (agent.user_policy), or its settings file changed while it ran
// (agent.settings_changed). Each is the question and its stop line, never a send's refusal, in place of the
// "U3-err-<code>" entry (registry-offline.tsx's ownEntry).
import { step, userMessage } from "../testing/stream-script.ts";
import type { ErrorCode } from "../transport/protocol.ts";
import type { Entry } from "./entry.ts";
import { threadHost } from "./hosts.ts";
import { RoomOver } from "./states.tsx";
import { errorWords } from "./words.ts";

/** A turn of Claude's that failed with `code`, on Windows, where the hooks and the policy key matter most. */
const stopped = (code: ErrorCode) => () => (
  <RoomOver
    host={() =>
      threadHost({
        entries: [
          userMessage(5, 1, "What does clause nine of the lease say about notice?"),
          step(9, { stop: "failed", failure: code }),
        ],
        platform: "windows",
      })
    }
  />
);

/** The entries in place of a refusal, by code. */
export const CLAUDE_SETTINGS: Partial<Record<ErrorCode, Entry[]>> = {
  "agent.firm_hooks_off": [
    {
      id: "UC-474",
      words: errorWords(
        "Your firm's Claude settings on this computer switch off a safety check Clauseo needs (the setting disableAllHooks or allowManagedHooksOnly), so Claude can't start here. Start this conversation with Codex instead.",
      ),
      screen: "room/thread",
      render: stopped("agent.firm_hooks_off"),
    },
  ],
  "agent.user_policy": [
    {
      id: "UC-475",
      words: errorWords(
        "A program on this computer changed Claude's security settings, so Clauseo won't run Claude until that change is gone. Uninstall the program that made it, then try again, or use Codex for now.",
      ),
      screen: "room/thread",
      render: stopped("agent.user_policy"),
    },
  ],
  "agent.settings_changed": [
    {
      id: "UC-476",
      words: errorWords(
        "Claude's settings changed while it was working, so Clauseo stopped it. Send your message again to restart Claude with Clauseo's own settings.",
      ),
      screen: "room/thread",
      render: stopped("agent.settings_changed"),
    },
  ],
};
