import { useWords } from "../lib/words.ts";

type ImageProps = { src?: string | undefined; alt?: string | undefined };

const DATA_IMAGE = /^data:image\/(?:png|jpeg|gif|webp);base64,[A-Za-z0-9+/]+=*$/;
/** 5 MB of image bytes, as base64 characters. */
const MAX_DATA_CHARS = Math.ceil((5 * 1024 * 1024 * 4) / 3);

function renderable(src: string | undefined): src is string {
  if (src === undefined) return false;
  if (src.startsWith("clauseo-file:")) return true;
  return src.length <= MAX_DATA_CHARS && DATA_IMAGE.test(src);
}

/**
 * Every image in an answer. Only clauseo-file: and inline raster data: images render; anything else
 * (remote images above all, the exfiltration channel) is a placeholder that fetches nothing. The sanitiser
 * already drops a src with any other scheme (schema.ts), so the placeholder has no URL to offer.
 */
export function Image({ src, alt }: ImageProps) {
  const words = useWords();
  if (renderable(src)) {
    return (
      <img
        data-clauseo="image"
        src={src}
        alt={alt ?? ""}
        referrerPolicy="no-referrer"
        loading="lazy"
        decoding="async"
      />
    );
  }
  return (
    <span data-clauseo="image" data-blocked="" role="img" aria-label={alt ?? ""}>
      {words.t("Image blocked")}
    </span>
  );
}
