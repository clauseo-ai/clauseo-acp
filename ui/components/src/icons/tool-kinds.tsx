// SPDX-License-Identifier: ISC
// One glyph per ACP tool kind, copied from https://github.com/lucide-icons/lucide icons/ at 1.66.0 (commit
// a630cec77eac6568b989e87e4947a8bfe45caacd) and re-cut on 3 Nov 2026 on a 20 px grid for a 1.5 px stroke with square
// ends and sharp joins, each drawn over its source and checked against it. Licence: LICENSES/ISC-lucide.txt. Named by
// kind; icons.tsx re-exports them, with the three kinds that reuse a shape it already draws (edit, search and fetch),
// and names.ts lists them.
import { Icon, type IconProps } from "./icon.tsx";

/** A tool call that reads files or data, not used in the room yet (source: bookOpen, Lucide book-open at 1.66.0; the
 * pages' curved edges drawn as straight cuts, so the spine's joins stay sharp and small) */
export function IconToolRead(props: IconProps) {
  return (
    <Icon {...props}>
      <path
        d="M10 4.95L7.1 3.5H2.75v11.6H7.1l2.9 1.45 2.9-1.45h4.35V3.5H12.9z"
        vectorEffect="non-scaling-stroke"
      />
      <path d="M10 4.95v11.55" vectorEffect="non-scaling-stroke" />
    </Icon>
  );
}

/** A tool call that deletes files, not used in the room yet (source: trash2, Lucide trash-2 at 1.66.0) */
export function IconToolDelete(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M8.55 9.25v4.35" vectorEffect="non-scaling-stroke" />
      <path d="M11.45 9.25v4.35" vectorEffect="non-scaling-stroke" />
      <path d="M15.05 5.65v11.6H4.95V5.65" vectorEffect="non-scaling-stroke" />
      <path d="M3.5 5.65h13" vectorEffect="non-scaling-stroke" />
      <path d="M7.1 5.65v-2.9h5.8v2.9" vectorEffect="non-scaling-stroke" />
    </Icon>
  );
}

/** A tool call that moves or renames files, not used in the room yet (source: arrowRightLeft, Lucide arrow-right-left
 * at 1.66.0) */
export function IconToolMove(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M12.9 3.5l2.9 2.9-2.9 2.9" vectorEffect="non-scaling-stroke" />
      <path d="M14.8 6.4H4.2" vectorEffect="non-scaling-stroke" />
      <path d="M7.1 16.5l-2.9-2.9 2.9-2.9" vectorEffect="non-scaling-stroke" />
      <path d="M5.2 13.6h10.6" vectorEffect="non-scaling-stroke" />
    </Icon>
  );
}

/** A tool call that runs a command, not used in the room yet (source: terminal, Lucide terminal at 1.66.0) */
export function IconToolExecute(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M10 15.1h5.8" vectorEffect="non-scaling-stroke" />
      <path d="M4.2 13.65l4.35-4.35-4.35-4.35" vectorEffect="non-scaling-stroke" />
    </Icon>
  );
}

/** A tool call that thinks or plans, with no side effect, not used in the room yet (source: lightbulb, Lucide
 * lightbulb at 1.66.0) */
export function IconToolThink(props: IconProps) {
  return (
    <Icon {...props}>
      <path
        d="M12.2 11.45c.15-.75.5-1.25 1.1-1.8.7-.65 1.05-1.6 1.05-2.55A4.35 4.35 0 0 0 5.65 7.1c0 .7.15 1.6 1.1 2.55.5.5.95 1.1 1.1 1.8"
        vectorEffect="non-scaling-stroke"
      />
      <path d="M7.85 14.35h4.3" vectorEffect="non-scaling-stroke" />
      <path d="M8.55 17.25h2.9" vectorEffect="non-scaling-stroke" />
    </Icon>
  );
}

/** A tool call that switches the session's mode, not used in the room yet (source: toggleRight, Lucide toggle-right
 * at 1.66.0; the switch's round ends kept, as a circle keeps its round: only corners are drawn square) */
export function IconToolSwitchMode(props: IconProps) {
  return (
    <Icon {...props}>
      <path
        d="M7.8 4.95h4.4a5.05 5.05 0 0 1 0 10.1H7.8a5.05 5.05 0 0 1 0-10.1z"
        vectorEffect="non-scaling-stroke"
      />
      <circle cx="12.2" cy="10" r="2.2" vectorEffect="non-scaling-stroke" />
    </Icon>
  );
}

/** Any other tool call, not used in the room yet (source: wrench, Lucide wrench at 1.66.0; the handle's round end
 * drawn square) */
export function IconToolOther(props: IconProps) {
  return (
    <Icon {...props}>
      <path
        d="M11.45 6.375L13.625 8.55 16.87 5.31A4.35 4.35 0 0 1 11.11 11.07L4.86 17.32 2.68 15.14 8.93 8.89A4.35 4.35 0 0 1 14.69 3.13z"
        vectorEffect="non-scaling-stroke"
      />
    </Icon>
  );
}
