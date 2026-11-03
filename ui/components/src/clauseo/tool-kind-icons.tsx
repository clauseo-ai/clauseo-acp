// The icon of a step in the thread, by what it does: ACP's tool kinds (read, edit, delete, move, search, execute,
// think, fetch, and other for the rest). Placeholders, drawn on the icons' own grid (icons/icon.tsx: 20 units, a
// 1.5 px stroke, square ends) until the icon pass brings the real glyphs; swapping them is a change to this file only:
// everything draws a step's icon through ToolKindIcon.
import { Icon, type IconProps } from "../icons/icon.tsx";
import { stepKindOf } from "./tool-kinds.ts";

const STROKE = { vectorEffect: "non-scaling-stroke" } as const;

function IconStepRead(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M11.5 2.5H4.5v15h11V6.5z" {...STROKE} />
      <path d="M7.5 9.5h5M7.5 12.5h5" {...STROKE} />
    </Icon>
  );
}

function IconStepEdit(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M4 16l.75-3.75L13 4l3 3-8.25 8.25z" {...STROKE} />
      <path d="M11 6l3 3" {...STROKE} />
    </Icon>
  );
}

function IconStepDelete(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M3.5 5.5h13M8 5.5V3.5h4v2" {...STROKE} />
      <path d="M5.5 5.5l.75 11h7.5l.75-11" {...STROKE} />
    </Icon>
  );
}

function IconStepMove(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M3.5 10h11" {...STROKE} />
      <path d="M11 6.5l3.5 3.5-3.5 3.5" {...STROKE} />
      <path d="M17 4.5v11" {...STROKE} />
    </Icon>
  );
}

function IconStepSearch(props: IconProps) {
  return (
    <Icon {...props}>
      <circle cx="8.75" cy="8.75" r="5.25" {...STROKE} />
      <path d="M12.5 12.5l4 4" {...STROKE} />
    </Icon>
  );
}

function IconStepExecute(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M2.5 3.5h15v13h-15z" {...STROKE} />
      <path d="M6 8l2.5 2L6 12M10.5 12.5h3.5" {...STROKE} />
    </Icon>
  );
}

function IconStepThink(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M10 2.75a5 5 0 0 0-3 9v2.5h6v-2.5a5 5 0 0 0-3-9z" {...STROKE} />
      <path d="M8 17.25h4" {...STROKE} />
    </Icon>
  );
}

function IconStepFetch(props: IconProps) {
  return (
    <Icon {...props}>
      <circle cx="10" cy="10" r="7.25" {...STROKE} />
      <path
        d="M2.75 10h14.5M10 2.75c-3.5 4.5-3.5 10 0 14.5M10 2.75c3.5 4.5 3.5 10 0 14.5"
        {...STROKE}
      />
    </Icon>
  );
}

function IconStepOther(props: IconProps) {
  return (
    <Icon {...props}>
      <circle cx="10" cy="10" r="7.25" {...STROKE} />
      <path d="M6.5 10h.5M9.75 10h.5M13 10h.5" {...STROKE} />
    </Icon>
  );
}

/** A tool call's icon, by its kind (other for a kind it doesn't know). */
export function ToolKindIcon({ kind, ...props }: IconProps & { kind: string }) {
  switch (stepKindOf(kind)) {
    case "read":
      return <IconStepRead {...props} />;
    case "edit":
      return <IconStepEdit {...props} />;
    case "delete":
      return <IconStepDelete {...props} />;
    case "move":
      return <IconStepMove {...props} />;
    case "search":
      return <IconStepSearch {...props} />;
    case "execute":
      return <IconStepExecute {...props} />;
    case "think":
      return <IconStepThink {...props} />;
    case "fetch":
      return <IconStepFetch {...props} />;
    case "other":
      return <IconStepOther {...props} />;
  }
}
