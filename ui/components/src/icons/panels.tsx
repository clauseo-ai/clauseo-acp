// SPDX-License-Identifier: ISC
// The toolbar's two panel toggles, copied from https://github.com/lucide-icons/lucide icons/ at 1.66.0 (panel-left and
// panel-right: a window with a column ruled off at one side) and re-cut on 15 Nov 2026 on a 20 px grid for a 1.5 px
// stroke with square ends and sharp joins, the rounded corners drawn square, in the outline IconMatters draws, so the
// sidebar's toggle and Matters' old button read the same. Licence: LICENSES/ISC-lucide.txt. icons.tsx re-exports
// them, and names.ts lists them.
import { Icon, type IconProps } from "./icon.tsx";

/** The sidebar: shows or hides the rail (source: panelLeft, Lucide panel-left at 1.66.0) */
export function IconSidebarLeft(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M2.75 3.25h14.5v13.5H2.75z" vectorEffect="non-scaling-stroke" />
      <path d="M7.75 3.25v13.5" vectorEffect="non-scaling-stroke" />
    </Icon>
  );
}

/** The papers: shows or hides the wall (source: panelRight, Lucide panel-right at 1.66.0) */
export function IconSidebarRight(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M2.75 3.25h14.5v13.5H2.75z" vectorEffect="non-scaling-stroke" />
      <path d="M12.25 3.25v13.5" vectorEffect="non-scaling-stroke" />
    </Icon>
  );
}
