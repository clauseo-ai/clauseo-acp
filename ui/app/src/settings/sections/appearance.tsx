// Settings, Appearance: the light, the text size, lite and reduced motion, each one patch to the display store per
// choice. The settings keep the room's spelling (light, dark; lite, full; reduced, full); the switches show the
// design's (Day, Night; On, Off). Lite offers On and Off only until the host reports remote desktops (H7's
// GetDisplayReport): "Automatic"'s line can't be true before then, so a stored "automatic" shows as Off. Text size is
// a native <select>, 14 to 20 (D-063): its label names it, a VoiceOver press opens it, and the keyboard works it as
// the OS's own pop-up does.
import { useId } from "react";
import { TEXT_SIZES } from "@clauseo/components/lib/display.ts";
import { Chip } from "@clauseo/components/ui/chip.tsx";
import { cn } from "@clauseo/components/lib/cn.ts";
import { PREVIEW_PAPERS, useSettingsWords } from "@clauseo/components/lib/words-settings.ts";
import { settingLabel, settingNote } from "@clauseo/components/ui/recipes-controls.ts";
import { SegmentedSwitch } from "@clauseo/components/ui/segmented.tsx";
import { IconMore as IconChevron } from "@clauseo/components/icons/icons.tsx";
import { selectTrigger } from "@clauseo/components/ui/recipes-overlays.ts";
import { RadioSetting } from "@clauseo/components/ui/settings-row.tsx";
import type { UiMotion, UiTheme } from "../../room/host-next.ts";
import type { SectionContext } from "../sections.tsx";
import { ChoiceRow, Group, Managed, PlateRow } from "./parts.tsx";

type OnOff = "on" | "off";

/** Reduced motion's switch, in the design's words, over the room's spelling. */
const MOTION_SHOWN: Record<UiMotion, "system" | OnOff> = {
  system: "system",
  reduced: "on",
  full: "off",
};
const MOTION_SET: Record<"system" | OnOff, UiMotion> = {
  system: "system",
  on: "reduced",
  off: "full",
};

/** The room in small, as the choices below make it: three papers on the wall and the answer's first line on the
 * work, in this page's own light, size, lite and motion. Hidden from assistive technology: it shows, it says nothing
 * the controls don't. */
function Preview() {
  const w = useSettingsWords();
  return (
    <div className="set-preview" aria-hidden="true">
      <div className="set-preview-wall">
        {PREVIEW_PAPERS.map((p) => (
          <span key={p.num} className="set-preview-tile" data-state={p.role}>
            <span className="set-preview-num">{p.num}</span>
            <span className="set-preview-title" lang="en">
              {p.title}
            </span>
            {p.role !== undefined && <Chip role={p.role} ground="tile" />}
          </span>
        ))}
      </div>
      <p className="set-preview-work">
        {w.say(w.s("A strong case, because the complaint names Mr. Sharma only as a director."))}
      </p>
    </div>
  );
}

export function Appearance({ ctx }: { ctx: SectionContext }) {
  const w = useSettingsWords();
  const { ui } = ctx.settings;
  const theme = ctx.locked("theme");
  const size = ctx.locked("font_size");
  const lite = ctx.locked("lite");
  const motion = ctx.locked("motion");
  // each locked row's managed line, named by its control
  const managed = { theme: useId(), size: useId(), lite: useId(), motion: useId() };
  const sizeId = useId();
  const sizeNote = useId();
  return (
    <>
      <Group title={w.s("How it looks")} plate={false}>
        <Preview />
      </Group>
      <Group>
        <ChoiceRow label={w.f("Theme")} locked={theme} managedId={managed.theme}>
          <SegmentedSwitch<UiTheme>
            label={w.f("Theme")}
            value={ui.theme}
            disabled={theme}
            describedBy={theme ? managed.theme : undefined}
            onValueChange={(v) => ctx.choose("theme", v)}
            options={[
              { value: "system", label: w.f("Follow the computer") },
              { value: "light", label: w.s("Day") },
              { value: "dark", label: w.s("Night") },
            ]}
          />
        </ChoiceRow>
        <PlateRow className="set-choice" disabled={size}>
          <div className="set-choice-words">
            <label htmlFor={sizeId} className={cn(settingLabel, "block cursor-default")}>
              {w.say(w.f("Text size"))}
            </label>
            <p id={sizeNote} className={settingNote}>
              {w.say(w.f("Every word on screen grows or shrinks with it."))}
            </p>
            {size && <Managed id={managed.size} />}
          </div>
          <span className="relative inline-flex flex-none">
            <select
              id={sizeId}
              value={String(ui.font_size)}
              disabled={size}
              aria-describedby={size ? `${sizeNote} ${managed.size}` : sizeNote}
              onChange={(e) => {
                const n = TEXT_SIZES.find((x) => String(x) === e.currentTarget.value);
                if (n !== undefined) ctx.choose("font_size", n);
              }}
              className={cn(selectTrigger, "appearance-none min-w-control-large pe-s8")}
              data-testid="text-size"
            >
              {TEXT_SIZES.map((n) => (
                <option key={n} value={String(n)}>
                  {n}
                </option>
              ))}
            </select>
            <span
              aria-hidden="true"
              className="pointer-events-none absolute inset-y-0 end-s3 flex items-center text-muted"
            >
              <IconChevron size={16} />
            </span>
          </span>
        </PlateRow>
        <PlateRow disabled={lite}>
          <RadioSetting<OnOff>
            className="py-0"
            label={w.ui("Lite mode")}
            note={w.ui("Takes away every shadow and all motion.")}
            value={ui.lite === "lite" ? "on" : "off"}
            disabled={lite}
            describedBy={lite ? managed.lite : undefined}
            onValueChange={(v) => ctx.choose("lite", v === "on" ? "lite" : "full")}
            choices={[
              { value: "on", label: w.ui("On"), note: w.ui("Always, on this computer") },
              { value: "off", label: w.ui("Off"), note: w.ui("Never, even over a remote desktop") },
            ]}
          />
          {lite && <Managed id={managed.lite} className="mt-s3" />}
        </PlateRow>
        <ChoiceRow
          label={w.f("Reduced motion")}
          note={w.f("Every change lands at once, and nothing moves.")}
          locked={motion}
          managedId={managed.motion}
        >
          <SegmentedSwitch<"system" | OnOff>
            label={w.f("Reduced motion")}
            value={MOTION_SHOWN[ui.motion]}
            disabled={motion}
            describedBy={motion ? managed.motion : undefined}
            onValueChange={(v) => ctx.choose("motion", MOTION_SET[v])}
            options={[
              { value: "system", label: w.f("Follow the computer") },
              { value: "on", label: w.ui("On") },
              { value: "off", label: w.ui("Off") },
            ]}
          />
        </ChoiceRow>
      </Group>
    </>
  );
}
