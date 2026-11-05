// SPDX-License-Identifier: ISC
// The web tools' glyphs, copied from https://github.com/lucide-icons/lucide icons/ at 1.66.0 (commit
// a630cec77eac6568b989e87e4947a8bfe45caacd) and re-cut on 3 Nov 2026 on a 20 px grid for a 1.5 px stroke with square
// ends and sharp joins, each drawn over its source and checked against it. Licence: LICENSES/ISC-lucide.txt.
// icons.tsx re-exports them, and names.ts lists them.
import { Icon, type IconProps } from "./icon.tsx";

/** The browser, Clauseo's own on this computer (source: appWindow, Lucide app-window at 1.66.0: a window with its top
 * bar, corners drawn square; the bar's ticks at x.75, so each 1.5 px stroke starts on a pixel at 1x) */
export function IconBrowser(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M2.75 4.25h14.5v11.5H2.75z" vectorEffect="non-scaling-stroke" />
      <path d="M2.75 7.25h14.5" vectorEffect="non-scaling-stroke" />
      <path d="M5.75 4.25v3" vectorEffect="non-scaling-stroke" />
      <path d="M8.75 4.25v3" vectorEffect="non-scaling-stroke" />
    </Icon>
  );
}

/** Download, the browser's one download (source: download, Lucide download at 1.66.0: an arrow into a tray, the
 * shaft stopping short of the head; the shaft and the tray's sides at x.75 and its floor at y.75, so each 1.5 px
 * stroke starts on a pixel at 1x, the arrow centred over the tray) */
export function IconDownload(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M9.75 3.5V11" vectorEffect="non-scaling-stroke" />
      <path d="M6.25 8.5L9.75 12l3.5-3.5" vectorEffect="non-scaling-stroke" />
      <path d="M3.75 12.25v4.5h12v-4.5" vectorEffect="non-scaling-stroke" />
    </Icon>
  );
}
