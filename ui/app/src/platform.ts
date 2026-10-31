// The computer's OS, as the app shows it. Inside the desktop app the host says which OS it runs on (`display_info`);
// that report is the only source there. A page outside Tauri (the dev-only gallery, the dev server in a browser,
// tests) has no host to ask, and falls back to the webview's own navigator.
import type { Platform } from "@clauseo/components/lib/display.ts";
import { osOf } from "@clauseo/components/lib/words-ui.ts";
import type { HostTransport } from "./transport/host-transport.ts";

/** The OS from the webview's navigator: userAgentData.platform where there is one, else navigator.platform or the
 * user agent. The Mac where there is no navigator. For pages without a host only. */
export function guessPlatform(): Platform {
  const nav = globalThis.navigator as
    | (Navigator & { userAgentData?: { platform?: string } })
    | undefined;
  if (nav === undefined) return "mac";
  return osOf(nav.userAgentData?.platform || nav.platform || nav.userAgent);
}

/** The OS the host runs on. Only a transport that reaches no host (`"null"`) falls back to `guessPlatform`; a desktop
 * transport whose call fails rejects, so a wrong guess never stands in for the host's answer. */
export async function hostPlatform(transport: HostTransport): Promise<Platform> {
  if (transport.kind === "null") return guessPlatform();
  return (await transport.displayInfo()).platform;
}
