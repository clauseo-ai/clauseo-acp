// The computer's OS, as the app shows it until the host reports it. The host will say which OS it runs on, and that
// report replaces this guess; until then the webview's own navigator is the only witness.
import type { Platform } from "@clauseo/components/lib/display.ts";
import { osOf } from "@clauseo/components/lib/words-ui.ts";

/** The OS from the webview's navigator: userAgentData.platform where there is one, else navigator.platform or the
 * user agent. The Mac where there is no navigator. */
export function guessPlatform(): Platform {
  const nav = globalThis.navigator as
    | (Navigator & { userAgentData?: { platform?: string } })
    | undefined;
  if (nav === undefined) return "mac";
  return osOf(nav.userAgentData?.platform || nav.platform || nav.userAgent);
}
