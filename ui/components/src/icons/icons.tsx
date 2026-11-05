// SPDX-License-Identifier: ISC
// Copied from https://github.com/lucide-icons/lucide icons/ at an unrecorded release, re-cut on a 20 px grid for a
// 1.5 px stroke with square ends and sharp joins. The re-cut redrew the shapes, so no release matches them exactly,
// and the design recorded none. The newest release when the set was cut, on 27 Oct 2026, was 1.62.0 (commit
// 03f11e2d6adc319ea21e9a1d5820a99cd3767257). Licence: LICENSES/ISC-lucide.txt. Clauseo's own glyphs, in the same
// cut, are in glyphs.tsx under the repo's licence; they are re-exported here so every icon has one import. Every icon
// is listed in names.ts, and icons.test.ts fails when the two disagree.
// The icon pass's 21 (IconBreak to IconPlanLimit, at the end of this file) were cut from Lucide 1.66.0 (commit
// a630cec77eac6568b989e87e4947a8bfe45caacd) on 3 Nov 2026, each drawn over its source at that release and checked
// against it. IconArchived was cut from Lucide 1.66.0's archive (same commit) on 4 Nov 2026, the same way. The tool kinds' glyphs are in tool-kinds.tsx, cut the same way, and re-exported here; the three that
// reuse a shape already here (IconToolEdit, IconToolSearch, IconToolFetch) stay in this file. The web tools'
// IconBrowser and IconDownload are in web-tools.tsx, cut from Lucide 1.66.0 on 3 Nov 2026 the same way and re-exported
// here; IconInfo, at the end of this file, reuses the shape IconAbout draws.
// IconFailed was cut from Lucide 1.66.0 (triangle-alert, the same commit) on 4 Nov 2026 for U16a, the same way: its
// rounded corners drawn square, its triangle drawn in from the edge so the miter joins stay inside the 20 px box.
// IconSnooze was cut from Lucide 1.66.0's alarm-clock (the same commit) on 5 Nov 2026 for U16b: its points scaled about
// the centre by 0.725, as IconChecking's clock was, so the two clocks read as one family.
import { Icon, type IconProps } from "./icon.tsx";

export {
  IconAccept,
  IconFileEmail,
  IconFileGeneric,
  IconFileMarkdown,
  IconFilePdf,
  IconFileWord,
  IconPageTurn,
  IconPaperBookPage,
  IconSafeWorkspace,
} from "./glyphs.tsx";
export {
  IconToolDelete,
  IconToolExecute,
  IconToolMove,
  IconToolOther,
  IconToolRead,
  IconToolSwitchMode,
  IconToolThink,
} from "./tool-kinds.tsx";
export { IconBrowser, IconDownload } from "./web-tools.tsx";

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

/** Send, the arrow right, not used in the room yet (source: arrow, as arrowRight: Lucide's arrow-right is the shape
 * Turn to it already draws, so it reuses it) */
export function IconArrowRight(props: IconProps) {
  return <IconTurnToIt {...props} />;
}

/** Wake now, on the Snoozed shelf: back up into the list (cut for Send, not used for it yet) (source: arrowUp) */
export function IconArrowUp(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M10 16V6" vectorEffect="non-scaling-stroke" />
      <path d="M5.5 9.5L10 5l4.5 4.5" vectorEffect="non-scaling-stroke" />
    </Icon>
  );
}

/** Send, the paper plane, not used in the room yet (source: send) */
export function IconSend(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M17 3l-5 14-2.75-6.25L3 8z" vectorEffect="non-scaling-stroke" />
      <path d="M15.75 4.25l-6.5 6.5" vectorEffect="non-scaling-stroke" />
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

/** Failed: a conversation's answer failed, on the rail's status line (source: triangleAlert, Lucide triangle-alert at
 * 1.66.0) */
export function IconFailed(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M10 3.25l7.5 13.5h-15z" vectorEffect="non-scaling-stroke" />
      <path d="M10 8v3.5" vectorEffect="non-scaling-stroke" />
      <path d="M10 14.15h.01" vectorEffect="non-scaling-stroke" />
    </Icon>
  );
}

/** A break: the working line's lead when an answer stopped or failed, not used in the room yet (source: circleAlert,
 * Lucide circle-alert at 1.66.0) */
export function IconBreak(props: IconProps) {
  return (
    <Icon {...props}>
      <circle cx="10" cy="10" r="7.25" vectorEffect="non-scaling-stroke" />
      <path d="M10 7.1V10" vectorEffect="non-scaling-stroke" />
      <path d="M10 12.9h.01" vectorEffect="non-scaling-stroke" />
    </Icon>
  );
}

/** Offline, not used in the room yet (source: wifiOff, Lucide wifi-off at 1.66.0) */
export function IconOffline(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M10 15.8h.01" vectorEffect="non-scaling-stroke" />
      <path d="M7.45 13.2a3.625 3.625 0 0 1 5.1 0" vectorEffect="non-scaling-stroke" />
      <path d="M4.95 10.6a7.25 7.25 0 0 1 3.75-1.95" vectorEffect="non-scaling-stroke" />
      <path d="M15.05 10.6a7.25 7.25 0 0 0-1.45-1.1" vectorEffect="non-scaling-stroke" />
      <path d="M2.75 7.7a10.875 10.875 0 0 1 3.05-1.9" vectorEffect="non-scaling-stroke" />
      <path d="M17.25 7.7a10.875 10.875 0 0 0-8.2-2.75" vectorEffect="non-scaling-stroke" />
      <path d="M2.75 2.75l14.5 14.5" vectorEffect="non-scaling-stroke" />
    </Icon>
  );
}

/** A question for you (source: circleQuestionMark, Lucide circle-question-mark at
 * 1.66.0) */
export function IconQuestion(props: IconProps) {
  return (
    <Icon {...props}>
      <circle cx="10" cy="10" r="7.25" vectorEffect="non-scaling-stroke" />
      <path
        d="M7.85 7.85a2.2 2.2 0 0 1 4.25.7c0 1.45-2.2 2.2-2.2 2.2"
        vectorEffect="non-scaling-stroke"
      />
      <path d="M10 13.6h.01" vectorEffect="non-scaling-stroke" />
    </Icon>
  );
}

/** Try again, Restart, Retry now, Try now (source: rotateCcw, Lucide rotate-ccw at 1.66.0;
 * the outline of IconHistory without its hands, so Try again and Restore read as one family) */
export function IconTryAgain(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M3 10a7 7 0 1 0 2.05-4.95L3 7.1" vectorEffect="non-scaling-stroke" />
      <path d="M3 3.25V7.1h3.85" vectorEffect="non-scaling-stroke" />
    </Icon>
  );
}

/** Stop, and a stopped matter or conversation (source: square, Lucide square at 1.66.0,
 * filled, as Stop's pair to the arrow up: drawn at 9 of the 20 units, square cornered, since a solid shape weighs
 * more than a stroked one) */
export function IconStop(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M5.5 5.5h9v9h-9z" fill="currentColor" stroke="none" />
    </Icon>
  );
}

/** A cross: Don't allow, Not allowed (source: x, Lucide x at 1.66.0) */
export function IconCross(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M14.35 5.65l-8.7 8.7" vectorEffect="non-scaling-stroke" />
      <path d="M5.65 5.65l8.7 8.7" vectorEffect="non-scaling-stroke" />
    </Icon>
  );
}

/** A conversation: a version Clauseo made in one (source: messageSquare, Lucide message-
 * square at 1.66.0) */
export function IconConversation(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M2.75 3.5h14.5v11.55H5.65l-2.9 1.75z" vectorEffect="non-scaling-stroke" />
    </Icon>
  );
}

/** Answer ready (source: messageSquareDot, Lucide message-square-dot at 1.66.0) */
export function IconAnswerReady(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M10.5 3.5H2.75v13.3l2.9-1.75h11.6V10.2" vectorEffect="non-scaling-stroke" />
      <circle cx="15.05" cy="5.65" r="2.2" vectorEffect="non-scaling-stroke" />
    </Icon>
  );
}

/** A change you made outside Clauseo (source: penLine, Lucide pen-line at 1.66.0) */
export function IconYourChange(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M10.75 16.5h5.8" vectorEffect="non-scaling-stroke" />
      <path d="M13.5 3l3 3L6 16.5H3v-3z" vectorEffect="non-scaling-stroke" />
    </Icon>
  );
}

/** Papers imported (source: fileInput, Lucide file-input at 1.66.0) */
export function IconImported(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M4.5 9.25V2.5h7l4 4v11h-11V15" vectorEffect="non-scaling-stroke" />
      <path d="M11.5 2.5v4h4" vectorEffect="non-scaling-stroke" />
      <path d="M2.75 12.2H9" vectorEffect="non-scaling-stroke" />
      <path d="M7.85 10l2.2 2.2-2.2 2.2" vectorEffect="non-scaling-stroke" />
    </Icon>
  );
}

/** Compare, Compare with now (source: diff, Lucide diff at 1.66.0) */
export function IconCompare(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M10 3.5v10.1" vectorEffect="non-scaling-stroke" />
      <path d="M4.95 8.55h10.1" vectorEffect="non-scaling-stroke" />
      <path d="M4.95 16.5h10.1" vectorEffect="non-scaling-stroke" />
    </Icon>
  );
}

/** Settings: Language (source: languages, Lucide languages at 1.66.0) */
export function IconLanguage(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M4.95 7.1l4.35 4.35" vectorEffect="non-scaling-stroke" />
      <path d="M4.2 11.45l4.35-4.35 1.45-2.15" vectorEffect="non-scaling-stroke" />
      <path d="M2.75 4.95h8.7" vectorEffect="non-scaling-stroke" />
      <path d="M6.4 2.75h.7" vectorEffect="non-scaling-stroke" />
      <path d="M17.25 17.25L13.625 10 10 17.25" vectorEffect="non-scaling-stroke" />
      <path d="M11.45 14.35h4.35" vectorEffect="non-scaling-stroke" />
    </Icon>
  );
}

/** Settings: Appearance (source: contrast, Lucide contrast at 1.66.0; the half disc is
 * filled, as Lucide's 2 px stroke all but fills it) */
export function IconAppearance(props: IconProps) {
  return (
    <Icon {...props}>
      <circle cx="10" cy="10" r="7.25" vectorEffect="non-scaling-stroke" />
      <path
        d="M10 14.35a4.35 4.35 0 0 0 0-8.7z"
        fill="currentColor"
        vectorEffect="non-scaling-stroke"
      />
    </Icon>
  );
}

/** Settings: About (source: info, Lucide info at 1.66.0) */
export function IconAbout(props: IconProps) {
  return (
    <Icon {...props}>
      <circle cx="10" cy="10" r="7.25" vectorEffect="non-scaling-stroke" />
      <path d="M10 12.9V10" vectorEffect="non-scaling-stroke" />
      <path d="M10 7.1h.01" vectorEffect="non-scaling-stroke" />
    </Icon>
  );
}

/** Pin to the top (source: pin, Lucide pin at 1.66.0) */
export function IconPin(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M10 13.6v3.65" vectorEffect="non-scaling-stroke" />
      <path
        d="M7.8 5.65V9.1l-2.9 3.25v1.25h10.2v-1.25L12.2 9.1V5.65h.7v-2.9H7.1v2.9z"
        vectorEffect="non-scaling-stroke"
      />
    </Icon>
  );
}

/** Show in Finder, or in Explorer (source: folderOpen, Lucide folder-open at 1.66.0, on the
 * set's folder (IconFolder's tab and frame) rather than Lucide's) */
export function IconShowInFinder(props: IconProps) {
  return (
    <Icon {...props}>
      <path
        d="M5.25 13.5l2-4h10.25l-2.25 7H2.5v-12h5.25l1.75 2.25h6.25v2"
        vectorEffect="non-scaling-stroke"
      />
    </Icon>
  );
}

/** Locate folder, and a folder that moved (source: folderSearch, Lucide folder-search at
 * 1.66.0, on the set's folder (IconFolder's tab and frame) rather than Lucide's) */
export function IconLocateFolder(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M8.5 16.5h-6v-12h5.25l1.75 2.25h8v2.75" vectorEffect="non-scaling-stroke" />
      <circle cx="13.25" cy="14.25" r="2.25" vectorEffect="non-scaling-stroke" />
      <path d="M14.85 15.85l1.4 1.4" vectorEffect="non-scaling-stroke" />
    </Icon>
  );
}

/** Remove from the list (source: listX, Lucide list-x at 1.66.0) */
export function IconRemoveFromList(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M12.9 4.95H3.5" vectorEffect="non-scaling-stroke" />
      <path d="M9.25 10H3.5" vectorEffect="non-scaling-stroke" />
      <path d="M12.9 15.05H3.5" vectorEffect="non-scaling-stroke" />
      <path d="M12.55 8.2l3.6 3.6" vectorEffect="non-scaling-stroke" />
      <path d="M16.15 8.2l-3.6 3.6" vectorEffect="non-scaling-stroke" />
    </Icon>
  );
}

/** An agent's command, in the / menu, not used in the room yet (source: squareSlash, Lucide square-slash at 1.66.0;
 * the corners drawn square, as every rounded corner in the set is) */
export function IconCommand(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M3.5 3.5h13v13h-13z" vectorEffect="non-scaling-stroke" />
      <path d="M7.85 12.15l4.3-4.3" vectorEffect="non-scaling-stroke" />
    </Icon>
  );
}

/** Needs your permission (source: shieldQuestionMark, Lucide shield-question-mark at
 * 1.66.0; its question mark is IconQuestion's) */
export function IconNeedsPermission(props: IconProps) {
  return (
    <Icon {...props}>
      <path
        d="M15.8 10.75c0 3.6-2.55 5.4-5.8 6.55-3.25-1.15-5.8-2.95-5.8-6.55V4.95c1.45 0 3.55-.7 5.8-2.2 2.25 1.5 4.35 2.2 5.8 2.2z"
        vectorEffect="non-scaling-stroke"
      />
      <path
        d="M7.85 7.85a2.2 2.2 0 0 1 4.25.7c0 1.45-2.2 2.2-2.2 2.2"
        vectorEffect="non-scaling-stroke"
      />
      <path d="M10 13.6h.01" vectorEffect="non-scaling-stroke" />
    </Icon>
  );
}

/** The plan's limit (source: gauge, Lucide gauge at 1.66.0) */
export function IconPlanLimit(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M10 11.45l2.9-2.9" vectorEffect="non-scaling-stroke" />
      <path d="M3.72 15.07a7.25 7.25 0 1 1 12.56 0" vectorEffect="non-scaling-stroke" />
    </Icon>
  );
}

/** Archived: a conversation set aside, on a Find result (source: archive, Lucide archive at 1.66.0; the lid and the
 * box's corners drawn square) */
export function IconArchived(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M2.8 3.5h14.4v3.6H2.8z" vectorEffect="non-scaling-stroke" />
      <path d="M4.2 7.1v10.1h11.6V7.1" vectorEffect="non-scaling-stroke" />
      <path d="M8.55 10.6h2.9" vectorEffect="non-scaling-stroke" />
    </Icon>
  );
}

/** A tool call that edits files, not used in the room yet (source: penLine, Lucide pen-line, as toolEdit: the shape
 * IconYourChange draws, so it reuses it) */
export function IconToolEdit(props: IconProps) {
  return <IconYourChange {...props} />;
}

/** A tool call that searches, not used in the room yet (source: search, as toolSearch: the shape Search this matter
 * draws, so it reuses it) */
export function IconToolSearch(props: IconProps) {
  return <IconSearch {...props} />;
}

/** A tool call that fetches from the network, a web fetch or a web search, not used in the room yet (source: website,
 * Lucide globe, as toolFetch: the shape IconWebsite draws, so it reuses it) */
export function IconToolFetch(props: IconProps) {
  return <IconWebsite {...props} />;
}

/** Snooze: a conversation set aside until a time, on the rail (source: alarmClock, Lucide alarm-clock at 1.66.0) */
export function IconSnooze(props: IconProps) {
  return (
    <Icon {...props}>
      <circle cx="10" cy="10.75" r="5.8" vectorEffect="non-scaling-stroke" />
      <path d="M10 7.85v2.9l1.45 1.45" vectorEffect="non-scaling-stroke" />
      <path d="M4.95 3.5L2.75 5.65" vectorEffect="non-scaling-stroke" />
      <path d="M17.25 5.65l-2.2-2.15" vectorEffect="non-scaling-stroke" />
      <path d="M5.9 14.85L4.2 16.55" vectorEffect="non-scaling-stroke" />
      <path d="M14.1 14.85l1.7 1.7" vectorEffect="non-scaling-stroke" />
    </Icon>
  );
}

/** More about it: an info tip's button (source: info, Lucide info at 1.66.0, as info: the shape IconAbout draws, so it
 * reuses it) */
export function IconInfo(props: IconProps) {
  return <IconAbout {...props} />;
}
