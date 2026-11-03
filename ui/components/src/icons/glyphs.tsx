// Clauseo's own glyphs, drawn in the same cut as the Lucide shapes in icons.tsx (a 20 px grid, a 1.5 px stroke,
// square ends): the paper book page, accept, the page turn, the safe workspace, and the file types' letter badges
// (the set's page with a solid tab, its letters knocked out; approved by Rohan on 3 Nov 2026). They are the project's
// work, under the repo's licence; icons.tsx re-exports them.
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

/** A Word document (.docx, .doc): the page with W on its tab, not used in the room yet (source: fileWord) */
export function IconFileWord(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M5.25 7.85V2.5H12.25l4 4v11H10.95" vectorEffect="non-scaling-stroke" />
      <path d="M12.25 2.5v4h4" vectorEffect="non-scaling-stroke" />
      <path
        d="M2 9.6h7.2v7.2h-7.2zM3 10.7 4.2 10.7 4.55 13.6 5.15 11.9 6.05 11.9 6.65 13.6 7 10.7 8.2 10.7 7.5 15.7 6.3 15.7 5.6 13.7 4.9 15.7 3.7 15.7z"
        fill="currentColor"
        fillRule="evenodd"
        stroke="none"
      />
    </Icon>
  );
}

/** A PDF: the page with PDF on its tab, not used in the room yet (source: filePdf) */
export function IconFilePdf(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M5.25 7.85V2.5H12.25l4 4v1.35" vectorEffect="non-scaling-stroke" />
      <path d="M12.25 2.5v4h4" vectorEffect="non-scaling-stroke" />
      <path
        d="M2 9.6h12.6v7.2h-12.6zM3 10.7 6.2 10.7 6.2 13.8 4.2 13.8 4.2 15.7 3 15.7zM4.2 11.9 5 11.9 5 12.6 4.2 12.6zM6.8 10.7 9.1 10.7 10 11.6 10 14.8 9.1 15.7 6.8 15.7zM8 11.9 8.4 11.9 8.8 12.3 8.8 14.1 8.4 14.5 8 14.5zM10.6 10.7 13.6 10.7 13.6 11.9 11.8 11.9 11.8 12.7 13.2 12.7 13.2 13.9 11.8 13.9 11.8 15.7 10.6 15.7z"
        fill="currentColor"
        fillRule="evenodd"
        stroke="none"
      />
    </Icon>
  );
}

/** A Markdown file (.md): the page with MD on its tab, not used in the room yet (source: fileMarkdown) */
export function IconFileMarkdown(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M5.25 7.85V2.5H12.25l4 4v11H14.15" vectorEffect="non-scaling-stroke" />
      <path d="M12.25 2.5v4h4" vectorEffect="non-scaling-stroke" />
      <path
        d="M2 9.6h10.4v7.2h-10.4zM3 10.7 4.25 10.7 5.3 12.6 6.35 10.7 7.6 10.7 7.6 15.7 6.4 15.7 6.4 13 5.75 14.3 4.85 14.3 4.2 13 4.2 15.7 3 15.7zM8.2 10.7 10.5 10.7 11.4 11.6 11.4 14.8 10.5 15.7 8.2 15.7zM9.4 11.9 9.8 11.9 10.2 12.3 10.2 14.1 9.8 14.5 9.4 14.5z"
        fill="currentColor"
        fillRule="evenodd"
        stroke="none"
      />
    </Icon>
  );
}

/** An email (.eml, .msg): the page with an envelope on its tab, not used in the room yet (source: fileEmail) */
export function IconFileEmail(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M5.25 7.85V2.5H12.25l4 4v11H12.15" vectorEffect="non-scaling-stroke" />
      <path d="M12.25 2.5v4h4" vectorEffect="non-scaling-stroke" />
      <path
        d="M2 9.6h8.4v7.2h-8.4zM3 10.7 4.35 10.7 6.2 12.2 8.05 10.7 9.4 10.7 6.2 13.45z"
        fill="currentColor"
        fillRule="evenodd"
        stroke="none"
      />
    </Icon>
  );
}

/** Any other file: the page alone, not used in the room yet (source: fileGeneric) */
export function IconFileGeneric(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M11.5 2.5H4.5v15h11V6.5z" vectorEffect="non-scaling-stroke" />
      <path d="M11.5 2.5v4h4" vectorEffect="non-scaling-stroke" />
    </Icon>
  );
}
