// Each item's mark: the app's own glyphs for the built-ins (Clauseo Case Law draws the steps'
// case-law glyph, IconSearch, as tool-kind-icons.tsx does); for what the lawyer adds, the picture
// the host fetched (a site's icon, a GitHub owner's avatar, stage D's `icon`) or a glyph for how it
// came. The webview fetches nothing; no bundled third-party mark.
import { Icon, type IconProps } from "@clauseo/components/icons/icon.tsx";
import { IconFileWord, IconPaperBookPage } from "@clauseo/components/icons/glyphs.tsx";
import {
  IconArchived,
  IconCommand,
  IconFolder,
  IconImported,
  IconOpen,
  IconScratchMatter,
  IconSearch,
  IconToolEdit,
  IconWebsite,
} from "@clauseo/components/icons/icons.tsx";
import { IconBrowser } from "@clauseo/components/icons/web-tools.tsx";
import type { AddonFromKind, AddonView } from "./addons.ts";

/** A skill the lawyer added: a page, its ribbon hanging from the top. */
export function IconSkill(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M4.5 2.5h11v15h-11z" vectorEffect="non-scaling-stroke" />
      <path d="M8.5 2.5v6l2-1.5 2 1.5v-6" vectorEffect="non-scaling-stroke" />
      <path d="M7.5 12.5h5" vectorEffect="non-scaling-stroke" />
    </Icon>
  );
}

/** A connector the lawyer added: a plug, its two pins up. */
export function IconConnector(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M7.5 2.5v4M12.5 2.5v4" vectorEffect="non-scaling-stroke" />
      <path d="M5 6.5h10v3.5a5 5 0 0 1-10 0z" vectorEffect="non-scaling-stroke" />
      <path d="M10 15v2.5" vectorEffect="non-scaling-stroke" />
    </Icon>
  );
}

const BUILT_IN: Record<string, (p: IconProps) => React.JSX.Element> = {
  "legal-drafting": IconToolEdit,
  "legal-research": IconPaperBookPage,
  "matter-brief": IconScratchMatter,
  stetdoc: IconFileWord,
  "case-law": IconSearch,
  browser: IconBrowser,
};

/** An added item's glyph, by how it came: a skill by its source, a connector by how Clauseo reaches
 * it. */
const BY_FROM: Record<AddonFromKind, (p: IconProps) => React.JSX.Element> = {
  skills_sh: IconSkill,
  github: IconSkill,
  url: IconOpen,
  zip: IconArchived,
  folder: IconFolder,
  program: IconCommand,
  address: IconWebsite,
  import: IconImported,
};

/** An item's tile. Built-ins draw the app's glyph (bundled). Added items draw the picture the host
 * fetched (a site's icon, a GitHub owner's avatar) when there is one, else the glyph for how they
 * came. */
export function ItemTile({ addon, big = false }: { addon: AddonView; big?: boolean }) {
  const size = big ? 24 : 18;
  // only a picture the host sent whole: a data: URL never reaches out, whatever the CSP says
  if (addon.source === "added" && addon.icon?.startsWith("data:image/") === true)
    return (
      <span
        className="lib-tile"
        data-big={big ? "" : undefined}
        data-kind="picture"
        aria-hidden="true"
      >
        <img src={addon.icon} alt="" />
      </span>
    );
  const fallback = addon.kind === "skill" ? IconSkill : IconConnector;
  const Glyph =
    (addon.source === "built_in" ? BUILT_IN[addon.id] : undefined) ??
    (addon.from === undefined ? fallback : BY_FROM[addon.from.kind]);
  return (
    <span
      className="lib-tile"
      data-big={big ? "" : undefined}
      data-kind={addon.source}
      aria-hidden="true"
    >
      <Glyph size={size} />
    </span>
  );
}
