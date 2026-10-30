import {
  showDisplay,
  TEXT_SIZES,
  useDisplay,
  type Display,
} from "@clauseo/components/lib/display.ts";
import { useUiWords } from "@clauseo/components/lib/words-ui.ts";
import * as icons from "@clauseo/components/icons/icons.tsx";
import { LayerProvider } from "@clauseo/components/ui/layer.tsx";
import { SegmentedSwitch } from "@clauseo/components/ui/segmented.tsx";
import { Select } from "@clauseo/components/ui/select.tsx";
import { Switch } from "@clauseo/components/ui/switch.tsx";
import { ToastProvider } from "@clauseo/components/ui/toast.tsx";
import type { ReactNode } from "react";
import { Choices, Fields, Folds, Measures, Pressables, Settings } from "./controls.tsx";
import { Section } from "./layout.tsx";
import { LANGS, PLATFORMS, SIZES, THEMES } from "./options.ts";
import { Overlays } from "./overlays.tsx";

/** Every control of the design system in its main states, under a bar that switches the display. */
export function Gallery() {
  return (
    <ToastProvider>
      <LayerProvider>
        <div className="grid content-start gap-s6 min-h-screen bg-ground text-primary">
          <Bar />
          <main className="grid grid-cols-1 mid:grid-cols-2 items-start gap-s8 px-s6 pb-s10">
            <Pressables />
            <Fields />
            <Choices />
            <Settings />
            <Measures />
            <Folds />
            <Overlays />
            <Icons />
          </main>
        </div>
      </LayerProvider>
    </ToastProvider>
  );
}

function Labelled({ label, children }: { label: string; children: ReactNode }) {
  return (
    <span className="flex items-center gap-s2">
      <span className="deva-nudge">{label}</span>
      {children}
    </span>
  );
}

/** The display's switches: theme, language, lite, reduced motion, platform and text size. */
function Bar() {
  const d = useDisplay();
  const w = useUiWords();
  const set = (patch: Partial<Display>) => showDisplay({ ...d, ...patch });
  return (
    <header className="flex flex-wrap items-center gap-x-s6 gap-y-s3 px-s6 py-s3 text-small text-muted rule-under-band">
      <h1 lang="en" className="mr-auto font-en text-ui text-primary">
        Clauseo controls
      </h1>
      <SegmentedSwitch
        label={w.f("Theme")}
        value={d.theme}
        options={THEMES}
        onValueChange={(theme) => set({ theme })}
      />
      <SegmentedSwitch
        label="Language"
        value={d.lang}
        options={LANGS}
        onValueChange={(lang) => set({ lang })}
      />
      <Labelled label={w.ui("Lite mode")}>
        <Switch
          label={w.ui("Lite mode")}
          checked={d.render === "lite"}
          onCheckedChange={(on) => set({ render: on ? "lite" : "full" })}
        />
      </Labelled>
      <Labelled label={w.f("Reduced motion")}>
        <Switch
          label={w.f("Reduced motion")}
          checked={d.motion === "reduced"}
          onCheckedChange={(on) => set({ motion: on ? "reduced" : "full" })}
        />
      </Labelled>
      <SegmentedSwitch
        label="Platform"
        value={d.platform}
        options={PLATFORMS}
        onValueChange={(platform) => set({ platform })}
      />
      <Select
        label={w.f("Text size")}
        value={String(d.textSize)}
        options={SIZES}
        onValueChange={(v) =>
          set({ textSize: TEXT_SIZES.find((s) => String(s) === v) ?? d.textSize })
        }
      />
    </header>
  );
}

const ICONS = Object.entries(icons);

function Icons() {
  return (
    <Section title="Icons">
      <div className="grid grid-cols-4 gap-x-s4 gap-y-s3 w-full">
        {ICONS.map(([name, Icon]) => {
          return (
            <div key={name} className="flex items-center gap-s2 min-w-0 text-small text-muted">
              <span className="flex-none text-primary">
                <Icon size={20} />
              </span>
              <span lang="en" className="font-en truncate">
                {name.replace(/^Icon/, "")}
              </span>
            </div>
          );
        })}
      </div>
    </Section>
  );
}
