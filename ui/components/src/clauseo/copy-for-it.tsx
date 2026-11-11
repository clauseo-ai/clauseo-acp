// "Copy details for IT": the quiet secondary action beside a line only the firm can act on (the
// line itself ends where it says the firm manages it; this never takes its place). A press copies a
// short plain-text note for the firm's IT (words-first-launch.ts's itNote builds it from facts that
// hold no secret) and the button says "Message copied" for 2 s. Where the webview refuses the
// clipboard (no clipboard, or writeText rejected), the note shows under the button in a read-only
// field, selected, so it can be copied by hand, and the button never claims it was copied. A blocked
// download (first-launch/install.tsx) uses it too, named "Copy addresses" (UC-144), on any computer.
import { useEffect, useRef, useState } from "react";
import type { Lang } from "../lib/display.ts";
import { fl } from "../lib/words-first-launch.ts";
import { Button } from "../ui/button.tsx";
import { textControl } from "../ui/recipes-controls.ts";

/** How long the button says "Message copied", in ms. */
const COPIED_MS = 2000;

/** Writes the text to the clipboard; rejects where there is none or the webview refuses it. */
const writeClipboard = (text: string): Promise<void> =>
  navigator.clipboard === undefined
    ? Promise.reject(new Error("no clipboard"))
    : navigator.clipboard.writeText(text);

export function CopyForIt({
  lang,
  text,
  testId = "copy-for-it",
  className,
  size = "small",
  label: named,
}: {
  lang: Lang;
  /** The note for IT, built by the surface's details builder. */
  text: string;
  testId?: string;
  className?: string | undefined;
  size?: "small" | "default";
  /** The button's own name where it isn't a note for IT (a blocked download's "Copy addresses"). */
  label?: string | undefined;
}) {
  const [state, setState] = useState<"idle" | "copied" | "refused">("idle");
  const field = useRef<HTMLTextAreaElement>(null);
  useEffect(() => {
    if (state !== "copied") return;
    const timer = setTimeout(() => setState("idle"), COPIED_MS);
    return () => clearTimeout(timer);
  }, [state]);
  useEffect(() => {
    if (state !== "refused") return;
    field.current?.focus();
    field.current?.select();
  }, [state]);
  const label = named ?? fl(lang, "Copy details for IT");
  return (
    <>
      <Button
        kind="quiet"
        size={size}
        {...(className === undefined ? {} : { className })}
        data-testid={testId}
        data-copied={state === "copied" ? "" : undefined}
        onClick={() => {
          writeClipboard(text).then(
            () => setState("copied"),
            () => setState("refused"),
          );
        }}
      >
        {state === "copied" ? fl(lang, "Message copied") : label}
      </Button>
      {state === "refused" && (
        <textarea
          ref={field}
          readOnly
          aria-label={label}
          className={`${textControl} py-s2 mt-s2 resize-none text-small`}
          data-testid={`${testId}-text`}
          rows={Math.min(8, text.split("\n").length)}
          value={text}
        />
      )}
    </>
  );
}
