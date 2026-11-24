// Skills and connectors' row on the rail's foot, above the scratch matter, drawn as that row is:
// the shelf glyph, the place's name, and its count ("9 skills, 6 connectors"), with a yellow "New"
// tag until the lawyer first opens it. Whether it was opened is kept by seen.ts.
import "./library-entry.css";
import { Icon, type IconProps } from "@clauseo/components/icons/icon.tsx";
import { useDisplay } from "@clauseo/components/lib/display.ts";
import { lb } from "@clauseo/components/lib/words-library.ts";

/** The place, on the rail: two books on a shelf and one leaning. */
export function IconLibrary(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M3.5 3.5h3v13h-3zM8 5.5h3v11H8z" vectorEffect="non-scaling-stroke" />
      <path d="M12.75 6.25l2.9-.78 2.85 10.63-2.9.78z" vectorEffect="non-scaling-stroke" />
    </Icon>
  );
}

export function LibraryRow({
  note,
  current,
  fresh,
  onOpen,
}: {
  note: string;
  current: boolean;
  fresh: boolean;
  onOpen: () => void;
}) {
  const { lang } = useDisplay();
  return (
    <div className="room-scratch lib-entry">
      <button
        type="button"
        className="room-matter-row"
        aria-current={current ? "page" : undefined}
        data-testid="library-entry"
        onClick={onOpen}
      >
        <IconLibrary size={16} className="room-scratch-icon" />
        <span className="room-scratch-text">
          <span className="room-matter-name">{lb(lang, "Skills and connectors")}</span>
          {(note !== "" || fresh) && (
            <span className="room-matter-note lib-entry-note">
              {note}
              {fresh && <span className="lib-new">{lb(lang, "New")}</span>}
            </span>
          )}
        </span>
      </button>
    </div>
  );
}
