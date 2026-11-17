// Settings › Skills' scenes for the copy review (registry-uc-skills.tsx): the settings window open at Skills over the
// fake host (skills-fake.ts). Never shipped.
import type { Lang } from "@clauseo/components/lib/display.ts";
import { useEffect, useRef, useState } from "react";
import { SettingsOver } from "../../review/states-settings.tsx";
import { FakeSkills, PACKS } from "./skills-fake.ts";

/** The list, the empty list, and Add a skill pressed with the host refusing it with `code`. */
export type SkillsPage = "list" | "empty" | { refused: string };

const settle = () => new Promise((resolve) => setTimeout(resolve, 60));

/** The settings window at Skills in one page's state. */
export function SkillsOver({ page, lang }: { page: SkillsPage; lang: Lang }) {
  const [skills] = useState(() => {
    const fake = new FakeSkills(page === "empty" ? [] : PACKS);
    if (typeof page === "object") fake.refuseNext = page.refused;
    return fake;
  });
  const ran = useRef(false);
  useEffect(() => {
    if (ran.current || typeof page !== "object") return;
    ran.current = true;
    void (async () => {
      // the section's body is a lazy chunk: wait for it, up to 10 s
      for (
        let i = 0;
        i < 160 && document.querySelector('[data-testid="skills-add"]') === null;
        i += 1
      )
        await settle();
      document.querySelector<HTMLElement>('[data-testid="skills-add"]')?.click();
      await settle();
      (document.activeElement as HTMLElement | null)?.blur();
    })();
  }, [page]);
  return <SettingsOver section="skills" lang={lang} skills={skills} />;
}
