// The composer menu's rows' words: a command's, and a paper's. The row itself, its highlight and its press, is the
// menu's (composer-menu.tsx).
import type { ComponentType } from "react";
import { cn } from "../lib/cn.ts";
import { Mixed } from "../lib/mixed.tsx";
import type { IconProps } from "../icons/icon.tsx";
import { IconCommand } from "../icons/icons.tsx";
import { rowNote } from "../ui/recipes-overlays.ts";
import type { MentionItem, SlashCommand } from "./composer.tsx";

/** A command's row: its name and the input it takes, and what it does. */
export function CommandRow({ c }: { c: SlashCommand | undefined }) {
  if (c === undefined) return null;
  return (
    <>
      <IconCommand size={16} className="mt-s0-5 text-muted group-data-highlighted:text-primary" />
      <span className="min-w-0">
        <span className="block">
          <bdi>/{c.name}</bdi>
          {c.input_hint !== undefined && c.input_hint !== "" && (
            <span className="text-muted">
              {" "}
              <Mixed>{c.input_hint}</Mixed>
            </span>
          )}
        </span>
        {c.description !== "" && (
          <span className={rowNote}>
            <Mixed>{c.description}</Mixed>
          </span>
        )}
      </span>
    </>
  );
}

/** A paper's row: its file type's icon, its name, and where it stands in the matter's folder. */
export function PaperRow({
  m,
  Icon,
}: {
  m: MentionItem | undefined;
  Icon: ComponentType<IconProps>;
}) {
  if (m === undefined) return null;
  return (
    <>
      <Icon size={20} className="text-muted group-data-highlighted:text-primary" />
      <span className="min-w-0">
        <span className="block truncate">
          <bdi>{m.label}</bdi>
        </span>
        {m.detail !== undefined && m.detail !== "" && (
          <span className={cn(rowNote, "truncate")}>
            <bdi>{m.detail}</bdi>
          </span>
        )}
      </span>
    </>
  );
}
