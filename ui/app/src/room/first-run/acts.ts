// The first run's shared pieces, apart from first-run.tsx to keep it under the size limit: the test ids the brief
// decides, each plan's name and prices page, and the install line's buttons for a plan.
import type { Plan } from "@clauseo/components/clauseo/first-launch/gate.tsx";
import type { AgentId } from "../../transport/protocol.ts";
import { CODEX, type SetupState, type SetupStore } from "../../store/setup.ts";

/** The test ids the brief decides (UD1, "Decided here"). */
export const IDS = {
  install: {
    line: "install-foot",
    retry: "install-retry",
    proxy: "install-proxy",
    askIt: "install-ask-it",
    signIn: "install-sign-in",
    install: "install-again",
  },
  gate: {
    root: "signin",
    state: "signin-state",
    cancel: "signin-cancel",
    paste: "signin-paste",
    code: "signin-code",
    deviceCode: "signin-device-code",
    key: "signin-key",
    loginFound: "login-found",
    loginUse: "login-use",
    loginOther: "login-other",
    signInTo: "signin-plan",
  },
  done: "signin-done",
  counts: "counts-switch",
} as const;

/** Each plan's prices page, for an account with no plan (H11's open_external asks first). */
export const PLANS: Record<Plan, string> = {
  claude: "https://claude.com/pricing",
  chatgpt: "https://chatgpt.com/pricing",
};
export const NAME: Record<Plan, string> = { claude: "Claude", chatgpt: "ChatGPT" };
export const planOf = (agent: AgentId): Plan => (agent === CODEX ? "chatgpt" : "claude");

export const copyText = (text: string) => void navigator.clipboard?.writeText(text).catch(() => {});

/** The install line's buttons for `agent`. */
export const installActs = (store: SetupStore, agent: AgentId, copied: SetupState["copied"]) => ({
  retry: () => store.command({ retry_agent_install: { agent } }),
  // a plan whose install went missing after Clauseo installed it (#859): the normal install, as choosing its card
  install: () => store.command({ install_agent: { agent } }),
  proxy: () => store.command({ retry_agent_install: { agent, use_system_proxy: true } }),
  copy: copyText,
  copied,
  setCopied: (what: "log") => store.set({ copied: what }),
});
