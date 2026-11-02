// Settings, Appearance: the light, the text size, lite and reduced motion, each one patch to the display store per
// choice. The settings keep the room's spelling (light, dark; lite, full; reduced, full); the switches show the
// design's (Day, Night; On, Off). Lite offers On and Off only until the host reports remote desktops (H7's
// GetDisplayReport): "Automatic"'s line can't be true before then, so a stored "automatic" shows as Off.
import { TEXT_SIZES } from "@clauseo/components/lib/display.ts";
import { cn } from "@clauseo/components/lib/cn.ts";
import { useSettingsWords } from "@clauseo/components/lib/words-settings.ts";
import { settingLabel, settingNote } from "@clauseo/components/ui/recipes-controls.ts";
import { SegmentedSwitch } from "@clauseo/components/ui/segmented.tsx";
import { Select, SelectLabel } from "@clauseo/components/ui/select.tsx";
import { RadioSetting } from "@clauseo/components/ui/settings-row.tsx";
import type { UiMotion, UiTheme } from "../../room/host-next.ts";
import type { SectionContext } from "../sections.tsx";
import { ChoiceRow, Group, Managed, PlateRow } from "./parts.tsx";

type Size = `${(typeof TEXT_SIZES)[number]}`;
type OnOff = "on" | "off";

const SIZES = TEXT_SIZES.map((n) => ({ value: String(n) as Size, label: String(n) }));

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

export function Appearance({ ctx }: { ctx: SectionContext }) {
  const w = useSettingsWords();
  const { ui } = ctx.settings;
  const theme = ctx.locked("theme");
  const size = ctx.locked("font_size");
  const lite = ctx.locked("lite");
  const motion = ctx.locked("motion");
  return (
    <Group>
      <ChoiceRow label={w.f("Theme")} locked={theme}>
        <SegmentedSwitch<UiTheme>
          label={w.f("Theme")}
          value={ui.theme}
          disabled={theme}
          onValueChange={(v) => ctx.choose("theme", v)}
          options={[
            { value: "system", label: w.f("Follow the computer") },
            { value: "light", label: w.s("Day") },
            { value: "dark", label: w.s("Night") },
          ]}
        />
      </ChoiceRow>
      <PlateRow disabled={size}>
        <Select<Size>
          value={String(ui.font_size) as Size}
          options={SIZES}
          disabled={size}
          onValueChange={(v) => {
            const n = TEXT_SIZES.find((x) => String(x) === v);
            if (n !== undefined) ctx.choose("font_size", n);
          }}
          className="min-w-control-large"
        >
          <div className="set-choice-words">
            <SelectLabel className={cn(settingLabel, "cursor-default")}>
              {w.say(w.f("Text size"))}
            </SelectLabel>
            <p className={settingNote}>
              {w.say(w.f("Every word on screen grows or shrinks with it."))}
            </p>
            {size && <Managed />}
          </div>
        </Select>
      </PlateRow>
      <PlateRow disabled={lite}>
        <RadioSetting<OnOff>
          className="py-0"
          label={w.ui("Lite mode")}
          note={w.ui("Takes away every shadow and all motion.")}
          value={ui.lite === "lite" ? "on" : "off"}
          disabled={lite}
          onValueChange={(v) => ctx.choose("lite", v === "on" ? "lite" : "full")}
          choices={[
            { value: "on", label: w.ui("On"), note: w.ui("Always, on this computer") },
            { value: "off", label: w.ui("Off"), note: w.ui("Never, even over a remote desktop") },
          ]}
        />
        {lite && <Managed className="mt-s3" />}
      </PlateRow>
      <ChoiceRow
        label={w.f("Reduced motion")}
        note={w.f("Every change lands at once, and nothing moves.")}
        locked={motion}
      >
        <SegmentedSwitch<"system" | OnOff>
          label={w.f("Reduced motion")}
          value={MOTION_SHOWN[ui.motion]}
          disabled={motion}
          onValueChange={(v) => ctx.choose("motion", MOTION_SET[v])}
          options={[
            { value: "system", label: w.f("Follow the computer") },
            { value: "on", label: w.ui("On") },
            { value: "off", label: w.ui("Off") },
          ]}
        />
      </ChoiceRow>
    </Group>
  );
}
