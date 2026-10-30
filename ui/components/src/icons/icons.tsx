// SPDX-License-Identifier: ISC
// Copied from https://github.com/lucide-icons/lucide icons/, re-cut on a 20 px grid for a 1.5 px stroke with square
// ends and sharp joins. Four glyphs are Clauseo's own, in the same cut, not Lucide's: the paper book page (source
// paperBook), accept, the page turn (pageTurn) and the safe workspace (safeRoom). Licence: LICENSES/ISC-lucide.txt.
// Every icon is listed in names.ts; icons.test.ts fails when the two disagree.
import { Icon, type IconProps } from "./icon.tsx";

/** A fold (source: chevron) */
export function IconFold(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M7.5 5l5 5-5 5" vectorEffect="non-scaling-stroke" />
    </Icon>
  );
}

/** N more (source: chevronDown) */
export function IconMore(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M5 7.5l5 5 5-5" vectorEffect="non-scaling-stroke" />
    </Icon>
  );
}

/** List of dates (source: doc) */
export function IconListOfDates(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M11.5 2.5H4.5v15h11V6.5z" vectorEffect="non-scaling-stroke" />
      <path d="M11.5 2.5v4h4" vectorEffect="non-scaling-stroke" />
      <path d="M7.5 9.5h2" vectorEffect="non-scaling-stroke" />
      <path d="M7.5 12.25h5" vectorEffect="non-scaling-stroke" />
      <path d="M7.5 15h5" vectorEffect="non-scaling-stroke" />
    </Icon>
  );
}

/** Scratch matter (source: scratch) */
export function IconScratchMatter(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M11.5 2.5H4.5v15h11V6.5z" vectorEffect="non-scaling-stroke" />
      <path d="M11.5 2.5v4h4" vectorEffect="non-scaling-stroke" />
    </Icon>
  );
}

/** Checking (source: wait) */
export function IconChecking(props: IconProps) {
  return (
    <Icon {...props}>
      <circle cx="10" cy="10" r="7.25" vectorEffect="non-scaling-stroke" />
      <path d="M10 6.5V10l2.5 1.75" vectorEffect="non-scaling-stroke" />
    </Icon>
  );
}

/** Good law (source: check) */
export function IconGoodLaw(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M4.5 10.5l3.5 3.5 7.5-7.5" vectorEffect="non-scaling-stroke" />
    </Icon>
  );
}

/** A tick: the chosen row, a checked box, a thing done (source: check, as tick) */
export function IconTick(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M4.5 10.5l3.5 3.5 7.5-7.5" vectorEffect="non-scaling-stroke" />
    </Icon>
  );
}

/** Open (source: open) */
export function IconOpen(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M7 6.5h6.5V13" vectorEffect="non-scaling-stroke" />
      <path d="M6.5 13.5l6-6" vectorEffect="non-scaling-stroke" />
    </Icon>
  );
}

/** Turn to it (source: arrow) */
export function IconTurnToIt(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M4 10h10" vectorEffect="non-scaling-stroke" />
      <path d="M10.5 5.5L15 10l-4.5 4.5" vectorEffect="non-scaling-stroke" />
    </Icon>
  );
}

/** Back to the answer (source: back) */
export function IconBackToAnswer(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M16 10H6" vectorEffect="non-scaling-stroke" />
      <path d="M9.5 5.5L5 10l4.5 4.5" vectorEffect="non-scaling-stroke" />
    </Icon>
  );
}

/** Matters (source: panel) */
export function IconMatters(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M2.75 3.25h14.5v13.5H2.75z" vectorEffect="non-scaling-stroke" />
      <path d="M7.75 3.25v13.5" vectorEffect="non-scaling-stroke" />
    </Icon>
  );
}

/** Search this matter (source: search) */
export function IconSearch(props: IconProps) {
  return (
    <Icon {...props}>
      <circle cx="8.75" cy="8.75" r="5.5" vectorEffect="non-scaling-stroke" />
      <path d="M13.25 13.25l3.25 3.25" vectorEffect="non-scaling-stroke" />
    </Icon>
  );
}

/** History (source: history) */
export function IconHistory(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M3 10a7 7 0 1 0 2.05-4.95L3 7.1" vectorEffect="non-scaling-stroke" />
      <path d="M3 3.25V7.1h3.85" vectorEffect="non-scaling-stroke" />
      <path d="M10 6.5V10l2.5 1.75" vectorEffect="non-scaling-stroke" />
    </Icon>
  );
}

/** Undo (source: undo) */
export function IconUndo(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M7.25 11.5l-4-4 4-4" vectorEffect="non-scaling-stroke" />
      <path d="M4.5 7.5h7.25a4.5 4.5 0 0 1 0 9H9" vectorEffect="non-scaling-stroke" />
    </Icon>
  );
}

/** New matter (source: plus) */
export function IconNewMatter(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M10 4.5v11" vectorEffect="non-scaling-stroke" />
      <path d="M4.5 10h11" vectorEffect="non-scaling-stroke" />
    </Icon>
  );
}

/** Add papers (source: addPaper) */
export function IconAddPapers(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M11.5 2.5H4.5v15h11V6.5z" vectorEffect="non-scaling-stroke" />
      <path d="M11.5 2.5v4h4" vectorEffect="non-scaling-stroke" />
      <path d="M7.5 12.5h5" vectorEffect="non-scaling-stroke" />
      <path d="M10 10v5" vectorEffect="non-scaling-stroke" />
    </Icon>
  );
}

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

/** Not protected (source: notProtected) */
export function IconNotProtected(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M2.75 9.25h14.5v9h-14.5z" vectorEffect="non-scaling-stroke" />
      <path d="M6 9.25V6a4 4 0 0 1 7.75-1.4" vectorEffect="non-scaling-stroke" />
    </Icon>
  );
}

/** A folder beyond the matter (source: folder) */
export function IconFolder(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M2.5 4.5h5.25l1.75 2.25h8v9.75h-15z" vectorEffect="non-scaling-stroke" />
    </Icon>
  );
}

/** A website (source: website) */
export function IconWebsite(props: IconProps) {
  return (
    <Icon {...props}>
      <circle cx="10" cy="10" r="7.25" vectorEffect="non-scaling-stroke" />
      <path d="M2.75 10h14.5" vectorEffect="non-scaling-stroke" />
      <path
        d="M10 2.75c-2.25 2-3.25 4.5-3.25 7.25s1 5.25 3.25 7.25c2.25-2 3.25-4.5 3.25-7.25S12.25 4.75 10 2.75z"
        vectorEffect="non-scaling-stroke"
      />
    </Icon>
  );
}
