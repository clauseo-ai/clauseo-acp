// A paper's file type, by its name's extension, and the glyph that shows it: the wall's tiles at 24 px and the @ menu's
// rows at 20 px. Word (.doc, .docx), PDF, Markdown (.md, .markdown) and email (.eml, .msg); anything else, or a name with
// no extension, is the generic page. Case doesn't matter, and only the last dot counts.
import type { ComponentType } from "react";
import type { IconProps } from "./icon.tsx";
import {
  IconFileEmail,
  IconFileGeneric,
  IconFileMarkdown,
  IconFilePdf,
  IconFileWord,
} from "./icons.tsx";

export type FileType = "word" | "pdf" | "markdown" | "email" | "generic";

const BY_EXTENSION: Readonly<Record<string, FileType>> = {
  doc: "word",
  docx: "word",
  pdf: "pdf",
  md: "markdown",
  markdown: "markdown",
  eml: "email",
  msg: "email",
};

const GLYPH: Readonly<Record<FileType, ComponentType<IconProps>>> = {
  word: IconFileWord,
  pdf: IconFilePdf,
  markdown: IconFileMarkdown,
  email: IconFileEmail,
  generic: IconFileGeneric,
};

/** A file's type, by the extension after its name's last dot (in any folder, in any case). */
export function fileTypeOf(name: string): FileType {
  const base = name.slice(Math.max(name.lastIndexOf("/"), name.lastIndexOf("\\")) + 1).trim();
  const dot = base.lastIndexOf(".");
  // a name that is all extension (".eml") or ends in a dot has none
  if (dot <= 0 || dot === base.length - 1) return "generic";
  const ext = base.slice(dot + 1).toLowerCase();
  return Object.hasOwn(BY_EXTENSION, ext) ? (BY_EXTENSION[ext] ?? "generic") : "generic";
}

/** The glyph for a file by its name: the letter badge for Word, PDF and Markdown, the envelope for email, the bare page
 * for anything else. Draw it at 24 px on a wall tile and 20 px in a menu row, hidden from assistive technology: the
 * file's name names it. */
export function fileTypeIcon(name: string): ComponentType<IconProps> {
  return GLYPH[fileTypeOf(name)];
}
