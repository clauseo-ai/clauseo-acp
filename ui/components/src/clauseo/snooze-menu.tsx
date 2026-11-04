// The snooze menu's content (U16b), loaded later: "Snooze until" and its times, each a menu row ("6:00 pm" or
// "9:00 pm", "Tomorrow 9:00 am", "Monday 9:00 am", "Before the next date" with its day and time beneath), then
// "Choose a day and time", which opens the picker in the row's place: the system's own date and time field, with
// Snooze and Cancel. The room works out the times (room/snooze-times.ts).
import { useEffect, useRef, useState } from "react";
import { Button } from "../ui/button.tsx";
import { MenuGroup, MenuGroupLabel, MenuItem, MenuSeparator } from "../ui/menu.tsx";
import { IconCross } from "../icons/icons.tsx";
import { useDisplay } from "../lib/display.ts";
import { twl } from "../lib/words-triage-late.ts";
import { ui } from "../lib/words-ui.ts";

export type SnoozeChoice = { key: string; label: string; note?: string; onPick: () => void };

export function SnoozeItems({
  choices,
  onChoose,
}: {
  choices: readonly SnoozeChoice[];
  onChoose: () => void;
}) {
  const { lang } = useDisplay();
  return (
    <>
      <MenuGroup>
        <MenuGroupLabel>{twl(lang, "Snooze until")}</MenuGroupLabel>
        {choices.map((c) => (
          <MenuItem
            key={c.key}
            label={c.label}
            {...(c.note === undefined ? {} : { note: c.note })}
            onClick={c.onPick}
          />
        ))}
      </MenuGroup>
      <MenuSeparator />
      <MenuItem label={twl(lang, "Choose a day and time")} onClick={onChoose} />
    </>
  );
}

/** The picker in a row's place: a day and a time between `min` and `max` ("2026-11-05T17:30", the field's own form),
 * Snooze to send it, Cancel or Escape to leave it. Snooze stands greyed while the value isn't one the host takes
 * (`valid`): empty, already past, or too far ahead. */
export function SnoozePicker({
  min,
  max,
  valid,
  onPick,
  onCancel,
}: {
  min: string;
  max: string;
  valid: (value: string) => boolean;
  onPick: (value: string) => void;
  onCancel: () => void;
}) {
  const { lang } = useDisplay();
  const [value, setValue] = useState("");
  const field = useRef<HTMLInputElement>(null);
  useEffect(() => field.current?.focus(), []);
  const label = twl(lang, "Choose a day and time");
  return (
    <form
      className="room-picker"
      aria-label={label}
      onSubmit={(e) => {
        e.preventDefault();
        if (valid(value)) onPick(value);
      }}
      onKeyDown={(e) => {
        if (e.key !== "Escape") return;
        e.preventDefault();
        e.stopPropagation();
        onCancel();
      }}
    >
      <input
        ref={field}
        type="datetime-local"
        className="room-picker-field"
        aria-label={label}
        data-testid="snooze-picker"
        required
        min={min}
        max={max}
        value={value}
        onChange={(e) => setValue(e.target.value)}
      />
      <span className="room-picker-acts">
        <Button kind="tone" size="small" type="submit" disabled={!valid(value)}>
          {twl(lang, "Snooze")}
        </Button>
        <Button
          kind="quiet"
          size="small"
          shape="icon"
          aria-label={ui(lang, "Cancel")}
          title={ui(lang, "Cancel")}
          onClick={onCancel}
        >
          <IconCross size={16} />
        </Button>
      </span>
    </form>
  );
}
