// Copy (D-026), the one control every Copy in the thread is: under a finished answer and on each of its code blocks. A
// quiet square button holding the copy icon, named Copy and tipped Copy; once the words are on the clipboard the tick
// stands in the icon's place for COPIED_MS and a polite live region says Copied, then both go back. The words come
// from the caller, in the room's language, so this file holds none of its own.
import { useEffect, useRef, useState } from "react";
import { IconCopy, IconTick } from "../icons/icons.tsx";
import { cn } from "../lib/cn.ts";
import { Button } from "./button.tsx";
import { Tooltip } from "./tooltip.tsx";

/** How long the tick, and "Copied", stand in Copy's place. */
const COPIED_MS = 2_000;

export function CopyButton({
  text,
  label,
  done,
  testId,
  className,
}: {
  /** The words to copy, read on the press; nothing is copied when they are empty. */
  text: () => string;
  /** The button's name and tooltip: "Copy", in the room's words. */
  label: string;
  /** What the live region says once copied: "Copied", in the room's words. */
  done: string;
  testId?: string;
  className?: string;
}) {
  const [copied, setCopied] = useState(false);
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  useEffect(() => () => clearTimeout(timer.current), []);
  const copy = () => {
    const words = text();
    if (words === "" || navigator.clipboard === undefined) return;
    navigator.clipboard.writeText(words).then(
      () => {
        clearTimeout(timer.current);
        setCopied(true);
        timer.current = setTimeout(() => setCopied(false), COPIED_MS);
      },
      () => {},
    );
  };
  return (
    <span className={cn("inline-flex", className)} data-copied={copied ? "" : undefined}>
      <Tooltip content={label}>
        <Button
          kind="quiet"
          size="small"
          shape="icon"
          aria-label={label}
          data-testid={testId}
          onClick={copy}
        >
          {copied ? <IconTick size={16} /> : <IconCopy size={16} />}
        </Button>
      </Tooltip>
      {/* in the page from the start, so a reader hears the words that arrive in it */}
      <span className="sr-only" role="status" aria-live="polite">
        {copied ? done : ""}
      </span>
    </span>
  );
}
