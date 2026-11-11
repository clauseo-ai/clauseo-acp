// Settings › Tools' probe option (UC-169 to UC-189): the settings window open at Tools over the fake host (tools-fake.ts),
// in one of tools-scenes.ts's states, reached by pressing and typing as the lawyer would. The room's probe page renders one with
// ?tools=<page> (probe-main.tsx), for the shots; the copy review's entries (registry-mcp-tools.tsx) render the same
// scenes. Never shipped.
import type { Lang } from "@clauseo/components/lib/display.ts";
import { useEffect, useRef, useState } from "react";
import { SettingsOver } from "../../review/states-settings.tsx";
import { FakeTools } from "./tools-fake.ts";
import { TOOLS_SCENES, type ToolsPage, type ToolsScene, type ToolsStep } from "./tools-scenes.ts";

const settle = () => new Promise((resolve) => setTimeout(resolve, 60));

async function run(steps: readonly ToolsStep[]) {
  // the section's body is a lazy chunk: wait for it, up to 10 s
  for (
    let i = 0;
    i < 160 && document.querySelector('[data-testid="tools-section"]') === null;
    i += 1
  )
    await settle();
  await settle();
  for (const step of steps) {
    if ("click" in step) document.querySelector<HTMLElement>(step.click)?.click();
    else if ("words" in step)
      [...document.querySelectorAll<HTMLElement>("button")]
        .find((b) => b.textContent === step.words)
        ?.click();
    else if ("submit" in step)
      document.querySelector<HTMLFormElement>('[data-testid="tools-form"]')?.requestSubmit();
    else {
      const field = document.querySelector<HTMLInputElement | HTMLTextAreaElement>(step.type[0]);
      if (field !== null) {
        const proto =
          field instanceof HTMLTextAreaElement
            ? HTMLTextAreaElement.prototype
            : HTMLInputElement.prototype;
        Reflect.set(proto, "value", step.type[1], field);
        field.dispatchEvent(new Event("input", { bubbles: true }));
      }
    }
    await settle();
  }
  // a shot shows the state, not the caret: focus leaves the last field
  (document.activeElement as HTMLElement | null)?.blur();
}

/** The settings window at Tools in one scene's state. */
export function ToolsSceneOver({ page, lang }: { page: ToolsPage; lang: Lang }) {
  const scene: ToolsScene = TOOLS_SCENES[page];
  const [tools] = useState(
    () => new FakeTools(scene.view, { previewRemote: scene.previewRemote ?? false }),
  );
  const ran = useRef(false);
  useEffect(() => {
    // once, though Strict Mode runs the effect twice
    if (ran.current) return;
    ran.current = true;
    void run(scene.steps ?? []);
  }, [scene]);
  return <SettingsOver section="tools" lang={lang} mcpTools={tools} />;
}
