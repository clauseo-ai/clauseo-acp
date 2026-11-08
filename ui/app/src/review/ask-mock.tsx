// A mock of R18's widening question for ND's copy review (registry-nd.tsx): the dialog Rust draws when Clauseo was
// stopped at the edge of the Safe workspace, which the webview never shows. Its classes are review.css's own, as
// confirm-mock.tsx's are.
import type { Lang } from "@clauseo/components/lib/display.ts";
import { useDisplay } from "@clauseo/components/lib/display.ts";
import { Button } from "@clauseo/components/ui/button.tsx";
import type { WordsRef } from "./words.ts";

/** The question's buttons, as dialogs/ask/words.rs names them. */
export type AskButtons = {
  allow: WordsRef;
  refuse: WordsRef;
  remember: WordsRef;
  allowOnce: WordsRef;
  allowAlways: WordsRef;
};

/** A widening question, mocked: its title and lines filled, then on Windows a Remember checkbox with Allow and Don't
 * allow; elsewhere Don't allow, Allow once and Always allow for this matter. */
export function AskMock({
  lang,
  title,
  lines,
  buttons,
}: {
  lang: Lang;
  title: string;
  lines: string[];
  buttons: AskButtons;
}) {
  const { platform } = useDisplay();
  const text = (ref: WordsRef) => ref.text(lang);
  return (
    <div className="review-confirm-ground">
      <div role="alertdialog" aria-label={title} className="review-confirm">
        <p>
          <strong>{title}</strong>
        </p>
        {lines.map((line) => (
          <p key={line}>{line}</p>
        ))}
        {platform === "windows" ? (
          <>
            <label>
              <input type="checkbox" readOnly /> {text(buttons.remember)}
            </label>
            <div className="review-confirm-buttons">
              <Button kind="primary">{text(buttons.allow)}</Button>
              <Button kind="tone">{text(buttons.refuse)}</Button>
            </div>
          </>
        ) : (
          <div className="review-confirm-buttons">
            <Button kind="tone">{text(buttons.refuse)}</Button>
            <Button kind="tone">{text(buttons.allowOnce)}</Button>
            <Button kind="primary">{text(buttons.allowAlways)}</Button>
          </div>
        )}
      </div>
    </div>
  );
}
