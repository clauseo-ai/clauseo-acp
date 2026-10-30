// Clauseo's own glyphs, drawn in the same cut as the Lucide shapes in icons.tsx (a 20 px grid, a 1.5 px stroke,
// square ends): the paper book page, accept, the page turn and the safe workspace. They are the project's work,
// under the repo's licence; icons.tsx re-exports them.
import { Icon, type IconProps } from "./icon.tsx";

/** Paper book page, not used in the room yet (source: paperBook) */
export function IconPaperBookPage(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M4.5 2.5h11v15h-11z" vectorEffect="non-scaling-stroke" />
      <path d="M7.25 2.5v15" vectorEffect="non-scaling-stroke" />
      <path d="M9.75 6.5h3" vectorEffect="non-scaling-stroke" />
      <path d="M9.75 9.25h3" vectorEffect="non-scaling-stroke" />
      <path d="M11.5 14.5h1.25" vectorEffect="non-scaling-stroke" />
    </Icon>
  );
}

/** Accept into the matter (source: accept) */
export function IconAccept(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M5 8.5l3 3 6.5-6.5" vectorEffect="non-scaling-stroke" />
      <path
        d="M4.5 14.75h11v2h-11z"
        fill="currentColor"
        stroke="none"
        vectorEffect="non-scaling-stroke"
      />
    </Icon>
  );
}

/** The prepared draft, waiting to be turned to (source: pageTurn) */
export function IconPageTurn(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M4.5 2.5h11v9l-6 6h-5z" vectorEffect="non-scaling-stroke" />
      <path d="M15.5 11.5h-6v6" vectorEffect="non-scaling-stroke" />
    </Icon>
  );
}

/** Safe workspace (source: safeRoom) */
export function IconSafeWorkspace(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M2.75 9.25h14.5v9h-14.5z" vectorEffect="non-scaling-stroke" />
      <path d="M6 9.25V6a4 4 0 0 1 8 0v3.25" vectorEffect="non-scaling-stroke" />
    </Icon>
  );
}
