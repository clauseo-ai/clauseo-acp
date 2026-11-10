// The scratch matter's line (QU.7 (a)), for the work to show at the top of the scratch matter: what it is, and that
// what's asked there is kept like any matter's. Drafted to promise only what holds before stage C: no Indian Kanoon,
// no statutes, nothing about other matters' papers. U3's work slot renders it while the scratch matter is open.
import { IconScratchMatter } from "@clauseo/components/icons/icons.tsx";
import { Say } from "@clauseo/components/clauseo/say.tsx";
import { mt } from "@clauseo/components/lib/words-matters.ts";
import { useWords } from "@clauseo/components/lib/words.ts";

// It stands in the reading column, as the thread's entries and the SAC note do: the measure, centred, with the
// thread's side padding, so its icon lines up with the matter title and the questions under it. Under 1000 px the
// centre is one column on the ground, and the line sits under the matter title at the header's own inset.
export function ScratchLine() {
  const { lang } = useWords();
  return (
    <p
      className="mx-auto box-content flex max-w-measure items-start gap-s1-5 px-thread-pad pt-s6 pb-s2 text-small text-quiet max-mid:mx-0 max-mid:max-w-none max-mid:px-s4 max-mid:pt-0 max-mid:pb-s4"
      data-testid="scratch-line"
    >
      <IconScratchMatter size={16} className="flex-none mt-s0-5" />
      <span className="min-w-0">
        <Say>
          {[
            mt(lang, "A matter for quick questions, with no papers of its own."),
            mt(lang, "Every question asked here is kept, with its versions, like any matter's."),
          ].join(" ")}
        </Say>
      </span>
    </p>
  );
}
