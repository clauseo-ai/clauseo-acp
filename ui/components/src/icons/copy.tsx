// SPDX-License-Identifier: ISC
// Copy's glyph, copied from https://github.com/lucide-icons/lucide icons/ at 1.66.0 (lucide-static@1.66.0's copy.svg:
// a 14 unit sheet at 8,8 over the top and left edges of one behind it) and re-cut on 11 Nov 2026 on a 20 px grid for a
// 1.5 px stroke with square ends and sharp joins: its points scaled about the centre by 0.725, as IconSettings' were,
// then set on x.75 so each stroke starts on a pixel at 1x, the rounded corners drawn square. Licence:
// LICENSES/ISC-lucide.txt. icons.tsx re-exports it, and names.ts lists it.
import { Icon, type IconProps } from "./icon.tsx";

/** Copy: puts the words on the clipboard (source: copy, Lucide copy at 1.66.0: the front sheet whole, the sheet behind
 * it as its left and top edges, each ending in a short stub as the source's rounded corners do) */
export function IconCopy(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M6.75 6.75v10.5h10.5V6.75z" vectorEffect="non-scaling-stroke" />
      <path d="M12.75 4.25V2.75h-10v10h1.5" vectorEffect="non-scaling-stroke" />
    </Icon>
  );
}
