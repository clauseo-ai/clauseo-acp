// Folder rights' reason on a permission card (FR6): in a matter whose folder's permissions the lawyer can't change,
// each pending "Editing files" card says why it asks, in a few words, the explanation behind an info icon. Part of
// the thread's chunk (permission-row.tsx), with its words, and shares nothing with the room's notice, so neither
// chunk loads the other's.
import { Say } from "@clauseo/components/clauseo/say.tsx";
import { IconAbout } from "@clauseo/components/icons/icons.tsx";
import { frc } from "@clauseo/components/lib/words-folder-card.ts";
import { useWords } from "@clauseo/components/lib/words.ts";
import { Tooltip } from "@clauseo/components/ui/tooltip.tsx";

/** An explanation behind an info icon, named with its words, which also show under the pointer and on focus. */
function FolderInfo({ words }: { words: string }) {
  return (
    <Tooltip content={words}>
      <button type="button" className="room-info" aria-label={words}>
        <IconAbout size={14} />
      </button>
    </Tooltip>
  );
}

/** "Can't set folder permissions", and why behind the info icon. */
export function FolderReason() {
  const { lang } = useWords();
  return (
    <span className="inline-flex items-center gap-(--s0-5)" data-testid="folder-reason">
      <Say>{frc(lang, "Can't set folder permissions")}</Say>
      <FolderInfo
        words={frc(
          lang,
          "Clauseo can't change this folder's permissions, so it asks before each edit.",
        )}
      />
    </span>
  );
}
