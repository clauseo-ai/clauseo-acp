// "Copy details for IT" where the firm forbids overriding the unsafe-site list (the no-override
// notice and the blocklist.override_off refusal, thread/site-blocked.tsx): the note copies the
// code, the blocked site as the notice shows it, and Clauseo's version (words-first-launch.ts's
// itNote). Never the matter, the thread or the page's path: only the site's host.
import type { Lang } from "@clauseo/components/lib/display.ts";
import { firmItLink, itNote } from "@clauseo/components/lib/words-first-launch.ts";
import { bl } from "@clauseo/components/lib/words-blocklist.ts";

const OFF = "blocklist.override_off";

// settings/version.ts's value, read from vite.config.ts's define here: importing that module would
// give the thread's chunk a shared chunk to load (vite-config.test.ts)
declare const __CLAUSEO_VERSION__: string;
const APP_VERSION: string = __CLAUSEO_VERSION__;

export const blocklistItDetails = {
  /** The note for a site the firm's policy keeps blocked; `host` is the notice's own. */
  overrideOff: (lang: Lang, host: string, version: string = APP_VERSION): string =>
    itNote(
      lang,
      bl(
        lang,
        "Clauseo blocked a site on its unsafe-site list, and the firm's policy has turned off allowing it.",
      ),
      { code: OFF, site: host, version, link: firmItLink(OFF) },
    ),
};
