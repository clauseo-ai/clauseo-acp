// Design B: the installer sets the Safe workspace up, so where it didn't (its Windows prompt
// declined, or a silent per-user install) the repair is to run the installer again. A personal
// install gets a button that opens the installed version's release page through open_external,
// whose native confirm asks first; the lawyer downloads and runs the installer. Once the page is
// open, Clauseo quits through quit_app (the app's normal quit, as Cmd+Q), so the installer can
// replace it; a declined confirm or a refused open leaves Clauseo open. The app itself never
// downloads or starts the installer. A test build's release page is deleted or never existed, so it
// gets no button.
import { APP_VERSION } from "../../settings/version.ts";
import type { HostTransport } from "../../transport/host-transport.ts";
import { quietly } from "../open.ts";

/** Stage F's release page for a version: every `v*` release stays published, so an older version
 * keeps resolving. Never `/releases/latest`: the installer must match the installed version. Null
 * for a version that isn't a plain X.Y.Z (a local or dev build), which has no page. */
export function installerPage(version: string = APP_VERSION): string | null {
  return /^\d+\.\d+\.\d+$/.test(version)
    ? `https://github.com/clauseo-ai/clauseo-acp/releases/tag/v${version}`
    : null;
}

/** Whether the display says this is a test build (main's `Display.testBuild`, the desktop's `e2e`
 * feature), read loosely: this base's Display doesn't have the field yet, and a display without it
 * is no test build. A dry-run build has no marker the UI can read, so it still shows the button. */
export const isTestBuild = (display: object): boolean =>
  (display as { testBuild?: unknown }).testBuild === true;

/** The page the repair's button opens, or null for no button: a test build, or a version with no
 * page. */
export const repairPage = (display: object, version: string = APP_VERSION): string | null =>
  isTestBuild(display) ? null : installerPage(version);

/** The open that is waiting for the lawyer's answer to the host's confirm, if one is: a second
 * click meanwhile gets the same promise and opens nothing more. */
let pending: Promise<void> | null = null;

/** Opens the page through open_external, then quits Clauseo through quit_app, but only when the
 * page opened (the lawyer accepted the confirm). A decline (`transport.unauthorized`) or any other
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
    transport.openExternal(url).then(() => {
      opened = true;
    }),
  );
  if (opened) await quietly("quit_app", transport.quitApp());
}
