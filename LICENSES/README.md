# Licences of copied code

Clauseo is GPL-3.0-or-later. Code copied from another project keeps that project's licence text in
this folder, one file per project, named after the licence and the project.

Each copied file starts with two lines (with the comment syntax of its language):

```
// SPDX-License-Identifier: MIT
// Copied from <repo> <path> at <commit>
```

| File | Project |
|---|---|
| [MIT-t3code.txt](MIT-t3code.txt) | T3 Code, https://github.com/pingdotgg/t3code |
| [ISC-lucide.txt](ISC-lucide.txt) | Lucide icons, https://github.com/lucide-icons/lucide: the shapes in `ui/components/src/icons/icons.tsx` |
| [OFL-1.1-anek.txt](OFL-1.1-anek.txt) | Anek fonts, https://github.com/EkType/Anek: the font files in `ui/app/public/fonts/`, which carry no header; a copy there, `OFL.txt`, ships with them in the built app |
| [CC-BY-SA-4.0-gfm-spec.txt](CC-BY-SA-4.0-gfm-spec.txt) | GitHub Flavored Markdown Spec, https://github.com/github/cmark-gfm `test/spec.txt` at 27d942c8b0a62d192f616e5bf3578f4b6a89e180: its 24 extension examples, test data only, in `ui/components/src/markdown/gfm-spec.json`, which carries no header (JSON has no comments); its `source` field names the file and commit |
| [MIT-micromark-extension-gfm-strikethrough.txt](MIT-micromark-extension-gfm-strikethrough.txt) | micromark-extension-gfm-strikethrough, https://github.com/micromark/micromark-extension-gfm-strikethrough `dev/lib/syntax.js` at a3a75cc000d5cb607ebef07ad76ff4dc2e0b3794: adapted for `==highlight==` in `ui/components/src/markdown/mark-syntax.ts` |
