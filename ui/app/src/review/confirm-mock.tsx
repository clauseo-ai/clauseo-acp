// A mock of the desktop's native link confirm (NSAlert, TaskDialog) for H11's copy review (registry-h11.tsx): the
// dialog Rust draws, which the webview never shows. Its classes are review.css's own: the review builds no Tailwind
// utilities from its own files.
import type { Lang } from "@clauseo/components/lib/display.ts";
import { useDisplay } from "@clauseo/components/lib/display.ts";
import { Button } from "@clauseo/components/ui/button.tsx";
import type { WordsRef } from "./words.ts";

/** The native confirm, mocked: title, message, and the two buttons in the platform's order (the Mac puts the default
 * button last, Windows first). */
export function ConfirmMock({
  lang,
  title,
  message,
  ok,
  cancel,
}: {
  lang: Lang;
  title: WordsRef;
  message: string;
  ok: WordsRef;
  cancel: WordsRef;
}) {
  const { platform } = useDisplay();
  const open = (
    <Button key="ok" kind="primary">
      {ok.text(lang)}
    </Button>
  );
  const no = (
    <Button key="cancel" kind="tone">
      {cancel.text(lang)}
    </Button>
  );
  return (
    <div className="review-confirm-ground">
      <div role="alertdialog" aria-label={title.text(lang)} className="review-confirm">
        <p>
          <strong>{title.text(lang)}</strong>
        </p>
        <p>{message}</p>
        <div className="review-confirm-buttons">
          {platform === "windows" ? [open, no] : [no, open]}
        </div>
      </div>
    </div>
  );
}
