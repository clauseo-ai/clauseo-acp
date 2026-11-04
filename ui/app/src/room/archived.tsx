// An archived conversation, opened from Find (U16a): the host refuses a question to it (command.invalid, "the thread
// is archived"), so the room offers none. Its entries stay as they are, and this line stands in the composer's place,
// led by the archive glyph Find's result wears.
import "./archived.css";
import { IconArchived } from "@clauseo/components/icons/icons.tsx";
import { Say } from "@clauseo/components/clauseo/say.tsx";
import { th } from "@clauseo/components/lib/words-thread.ts";
import { useWords } from "@clauseo/components/lib/words.ts";

/** The line in the composer's place while the open conversation is archived. */
export function ArchivedLine() {
  const { lang } = useWords();
  return (
    <p className="room-archived text-ui text-muted" data-testid="archived-line">
      <IconArchived size={16} />
      <Say>{th(lang, "Archived. Read only.")}</Say>
    </p>
  );
}
