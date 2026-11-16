// Whether the open conversation's plan runs on the lawyer's API key, from the setup store the first run keeps
// (store/setup.ts, one per transport): the composer's line then names the key ("Uses your Anthropic API key"), and a
// refusal (agent.auth_required) says the key wasn't accepted, never "Sign in first". False on a transport without the
// setup wire (a test, the review page's plain rooms) and before the plan's sign-in is heard.
import { useSyncExternalStore } from "react";
import type { Lang } from "@clauseo/components/lib/display.ts";
import { th } from "@clauseo/components/lib/words-thread.ts";
import type { HostTransport } from "../transport/host-transport.ts";
import type { AgentId } from "../transport/protocol.ts";
import { keyInUse, setupStoreOf, type SetupSource } from "../store/setup.ts";
import { hasSetup } from "./first-run/hold.ts";

const never = () => () => {};
const no = () => false;

/** Whether `agent` is signed in with an API key now; false for none (null). */
export function useKeyInUse(transport: HostTransport, agent: AgentId | null): boolean {
  const store =
    agent !== null && hasSetup(transport)
      ? setupStoreOf(transport as HostTransport & SetupSource)
      : null;
  const read =
    store === null || agent === null ? no : () => keyInUse(store.state().agents[agent]?.signIn);
  // the same read for the static render (App.test's renderToString): no setup heard yet gives false
  return useSyncExternalStore(store === null ? never : store.subscribe, read, read);
}

/** The composer's status line: the key by its maker while `keyed` names the plan's kind (UC-147, UC-148), else the
 * plan by name (U6-06). */
export function planLine(lang: Lang, plan: string, keyed: string | undefined): string {
  if (keyed === "claude") return th(lang, "Uses your Anthropic API key");
  if (keyed === "codex") return th(lang, "Uses your OpenAI API key");
  return th(lang, "Uses your {name} plan", { name: plan });
}
