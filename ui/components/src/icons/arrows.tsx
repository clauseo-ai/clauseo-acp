// SPDX-License-Identifier: ISC
// The up and down arrows, moved out of icons.tsx unchanged to keep it under the line limit. Like the rest of that set,
// copied from https://github.com/lucide-icons/lucide icons/ at an unrecorded release and re-cut on a 20 px grid for a
// 1.5 px stroke with square ends and sharp joins (see icons.tsx's header); the down arrow is the up arrow turned.
// Licence: LICENSES/ISC-lucide.txt. icons.tsx re-exports them, and names.ts lists them.
import { Icon, type IconProps } from "./icon.tsx";

/** Wake now, on the Snoozed shelf: back up into the list (cut for Send, not used for it yet) (source: arrowUp) */
export function IconArrowUp(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M10 16V6" vectorEffect="non-scaling-stroke" />
      <path d="M5.5 9.5L10 5l4.5 4.5" vectorEffect="non-scaling-stroke" />
    </Icon>
  );
}

/** Latest, over the composer: down to the thread's end (source: arrowUp, turned) */
export function IconArrowDown(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M10 4v10" vectorEffect="non-scaling-stroke" />
      <path d="M5.5 10.5L10 15l4.5-4.5" vectorEffect="non-scaling-stroke" />
    </Icon>
  );
}
