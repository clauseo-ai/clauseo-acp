// Design B: the installer sets the Safe workspace up, so where it didn't (its Windows prompt
// declined, or a silent per-user install) the repair is to run the installer again. A personal
// install gets a button that opens the installed version's release page through open_external,
// whose native confirm asks first; the lawyer downloads and runs the installer. Once the page is
// open, Clauseo quits through quit_app (the app's normal quit, as Cmd+Q), so the installer can
// replace it; a declined confirm or a refused open leaves Clauseo open. The app itself never
// downloads or starts the installer. The build's release mode picks the page: a release's `v*`
// tag, a test build's `test-v*` tag, and no button for a dry run, which has no release.
import type { Display, ReleaseMode } from "@clauseo/components/lib/display.ts";
import { APP_VERSION } from "../../settings/version.ts";
import type { HostTransport } from "../../transport/host-transport.ts";
import { quietly } from "../open.ts";

/** The tag prefix of each release mode's page; a dry run publishes nothing, so it has none. */
const TAG_PREFIX: Record<ReleaseMode, string | null> = { release: "v", test: "test-v", dry: null };

/** Stage F's release page for a version in a release mode: every release stays published, so an
 * older version keeps resolving. Never `/releases/latest`: the installer must match the installed
 * version. Null for a dry run, or for a version that isn't a plain X.Y.Z (a local or dev build),
 * which has no page. */
export function installerPage(
  version: string = APP_VERSION,
  mode: ReleaseMode = "release",
): string | null {
  const prefix = TAG_PREFIX[mode];
  return prefix !== null && /^\d+\.\d+\.\d+$/.test(version)
    ? `https://github.com/clauseo-ai/clauseo-acp/releases/tag/${prefix}${version}`
    : null;
}

/** The page the repair's button opens for the build's release mode (`Display.releaseMode`), or
 * null for no button: a dry run, or a version with no page. */
export const repairPage = (
  display: Pick<Display, "releaseMode">,
  version: string = APP_VERSION,
): string | null => installerPage(version, display.releaseMode);

/** The open that is waiting for the lawyer's answer to the host's confirm, if one is: a second
 * click meanwhile gets the same promise and opens nothing more. */
let pending: Promise<void> | null = null;

/** Opens the page through open_external, then quits Clauseo through quit_app, but only when the
 * page opened (the lawyer accepted the confirm, which this open always asks for: a plain web link
 * otherwise opens unasked, and one click would quit Clauseo, stopping its installs). A decline (`transport.unauthorized`) or any other
 * refusal says nothing and quits nothing. Resolves when the quit has been asked, or nothing was. */
export function openInstallerPage(transport: HostTransport, url: string): Promise<void> {
  pending ??= openThenQuit(transport, url).finally(() => {
    pending = null;
  });
  return pending;
}

async function openThenQuit(transport: HostTransport, url: string): Promise<void> {
  let opened = false;
  await quietly(
    "open_external",
    transport.openExternal(url, undefined, true).then(() => {
      opened = true;
    }),
  );
  if (opened) await quietly("quit_app", transport.quitApp());
}
