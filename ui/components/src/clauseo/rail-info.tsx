// An explanation behind an info icon on the rail (U16b): the icon, named with the words, which also show under the
// pointer and on focus, on a target of at least 24 px (`.room-info`, triage.css). Loaded with the open shelf and the open Done fold.
import { Tooltip } from "../ui/tooltip.tsx";
import { IconAbout } from "../icons/icons.tsx";

export function RailInfo({ words }: { words: string }) {
  return (
    <p className="room-fold-info">
      <Tooltip content={words}>
        <button type="button" className="room-info" aria-label={words}>
          <IconAbout size={14} />
        </button>
      </Tooltip>
    </p>
  );
}
