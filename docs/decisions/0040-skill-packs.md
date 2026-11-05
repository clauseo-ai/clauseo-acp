# 0040: Skill packs

## Context

R21 (decision record 0030) and R22 (0031) give both agents a fixed set of skills: R21's `stetdoc`
and R22's `legal-drafting`, `legal-research` and `matter-brief`. They are compiled in, rendered
once at app start into Claude's plugin at `<data>/profile/claude-plugin`, and listed in Codex's
`developer_instructions`. 0031 ends with "lawyer-made skills come back only with their own
decisions". This is that decision.

Rohan wants skill loading to be general, so that a later Clauseo Hub is just more folders. The host
and orchestrator agreed the shape on 2026-11-02: a pack is a folder with a manifest; extra packs
live under the data folder; `profile` checks them; policy decides which may load; the lawyer turns
packs on, globally and per matter; a pack that changed after it was turned on does not load. The
orchestrator approved the picks below on 2026-11-05 (`scratch-e/packs-plan.md`, section (b)), and
added one requirement from Rohan: loading packs must not slow a launch noticeably.

Three facts on main shape the code. The profile's skills are one set shared by every launch
(`ClauseoProfile::with_skills`). Every session loads the same plugin folder. The `ProfileHook`
methods take only the agent's id and read no file. Per-matter packs differ from launch to launch
and can change on disk while the app runs, so they cannot join the shared set or the shared
folder.

## Decision

### The pack

A pack is a folder holding `clauseo-pack.json`:

```json
{
  "format": 1,
  "id": "nda-kit",
  "name": "NDA review kit",
  "version": "1.2.0",
  "author": "Example Chambers",
  "source_url": "https://example.org/nda-kit",
  "licence": "CC-BY-4.0",
  "last_reviewed": "2026-10-20",
  "instruction_only": true,
  "skills": [{"name": "nda-review"}, {"name": "nda-redline"}]
}
```

- Every field is required, and an unknown key (at the top or in a skill) refuses the pack.
  `source_url` must be `https`; Clauseo never fetches it. `last_reviewed` is a calendar date
  (`clauseo_protocol::Date`). `name`, `version`, `author` and `licence` are display text, 1 to 200
  characters with no control character, and never go into an agent's instructions or the host log.
- **Layout.** `skills/<skill>/SKILL.md` and that skill's files, for each listed skill. At the top
  level, also only `README.md`, `LICENSE`, `LICENSE.md`, `LICENSE.txt` and `NOTICE`. Anything else,
  including a folder under `skills/` the manifest does not list (even an empty one), refuses the
  pack. The top-level files are hashed but not rendered.
- **The built-in pack** has the id `clauseo`. It is compiled in and holds R22's three skills, plus
  stetdoc's when it loaded. It is not a folder, it has no hash, and its skills keep their names and
  their plugin exactly as 0031 sets them.
- **Extra packs** live in `<data>/skill-packs/<id>/` (`ProfilePaths::skill_packs`). The folder's
  name must equal the manifest's `id`, compared byte for byte against what `read_dir` returns, so
  a disk that ignores case never matches `NDA-Kit` to `nda-kit`.

### Ids

A pack id matches `^[a-z0-9]+(-[a-z0-9]+)*$`, is 2 to 30 bytes, and is not a Windows device name.
`clauseo`, every `clauseo-*`, `builtin`, `stetdoc` and each name in `skills::builtin::BUILTIN` are
reserved. Pack skill names follow the same charset with no `--`. Since neither part may hold `--`,
the rendered name `<pack>--<skill>` splits one way only, never equals a built-in name (none holds
`--`), and must fit `NAME_MAX` (64 bytes). The rule is `PackId` in `protocol`
(`crates/protocol/src/details/packs.rs`); `profile::packs::is_pack_id` calls it, and a profile test
checks that it refuses each of profile's built-in names, so the two lists cannot drift.

### Validation (in `profile`)

A pack is read once, into memory. The checks, the hash, the script test and the render all use
those bytes, so what is checked is what is copied.

- **Paths.** Each path relative to the pack passes `skills::is_safe_path`: `/`-separated ASCII
  `[A-Za-z0-9._-]` parts, none hidden, none ending in `.`, no device name, so no `..`, root, drive,
  `\` or `:`. A name that is not UTF-8 is refused. Two paths that differ only in case refuse the
  pack. Each path is at most 160 bytes and crosses at most 8 folders.
- **Links.** Every entry is examined with `symlink_metadata` and never followed. A symlink refuses
  the pack; on Windows so does any reparse point (`FILE_ATTRIBUTE_REPARSE_POINT`: junctions,
  symlinks, and cloud placeholders, which a read would fetch from the network). A file with more
  than one hard link is refused: a hard link to `~/.ssh/id_rsa` would otherwise be copied into a
  folder the agent reads.
- **The open.** On Unix the walk goes by file descriptor (`rustix`): each folder is opened
  relative to its parent's descriptor with `O_DIRECTORY | O_NOFOLLOW` and listed from it, each
  entry is `lstat`ed and opened relative to the same descriptor with `O_NOFOLLOW`, and the open
  descriptor must be the device and inode the `lstat` saw, a plain file with one link. No path is
  resolved twice. On Windows each folder and file is opened with `FILE_FLAG_OPEN_REPARSE_POINT`,
  and the handle must show no reparse point, one link (`GetFileInformationByHandle`), and a final
  path (`GetFinalPathNameByHandleW`, normalized) equal to the pack folder's own plus the relative
  path. So a folder swapped for a link mid-walk reads nothing from outside.
- **Folders.** Each folder is counted against the 64-folder cap when the walk finds it, and the pack
  is refused at the 65th. A folder is opened only when the walk takes it from its stack, so the walk
  holds at most one folder open per depth level, never more than 10 with the pack's own, however
  many folders wait. Empty folders are not hashed, so without the cap a pack could add folders
  freely, slowing every launch, without counting as a change.
- **Scripts or executables.** A file is one unless its extension is on the allow-list (`.md .txt
  .json .yaml .yml .csv .tsv .docx .xlsx .pdf .png .jpg .jpeg .gif .webp`, compared without case;
  `LICENSE` and `NOTICE` with no extension) **and** it has no Unix exec bit, does not start with
  `#!`, and does not start with ELF, PE (`MZ`) or Mach-O magic. It fails closed: a `.xml` reference
  is refused, and so is a `.md` that starts with `MZ`. A pack holding one is refused unless
  `skills.allow_scripts` is true. A pack whose manifest says `instruction_only: true` is refused if
  it holds one, whatever the setting. Allowed script files are copied, never run, and never get the
  exec bit.
- **Clauseo never runs a pack's files.** `packs` starts no process (a test reads its sources).
  Claude keeps `disableSkillShellExecution`.
- **Shell injection.** Every text file is scanned for `` !` ``, as built-in skills are, and one hit
  refuses the pack. A file of a binary type on the allow-list (`.docx .xlsx .pdf .png .jpg .jpeg
  .gif .webp`) is not scanned **when its bytes start with that format's magic** (`PK\x03\x04` for
  `.docx` and `.xlsx`, `%PDF`, `\x89PNG`, `FF D8 FF` for JPEG, `GIF87a`/`GIF89a`, RIFF with `WEBP`
  at byte 8): its bytes are compressed or encoded, an agent never loads them as skill text, and
  random bytes hold those two bytes about once in 64 KiB, so a scan would refuse almost every pack
  with a template. A "binary" file without its magic, really text, is scanned.
- **Size.** At most 1 MiB per file, 8 MiB and 256 files per pack, 64 folders per pack (the pack's
  own not counted, on every OS), 64 KiB for `clauseo-pack.json`, 32 skills per pack, and 16 extra packs per launch (beyond 16, skipped as `TooMany`, in
  `skills.enabled` order). The size is checked from the metadata before the read and enforced again
  during it (the read stops one byte past the cap), so a file that grows mid-read is refused.
- **Frontmatter.** Each `SKILL.md` must be UTF-8, start with a frontmatter whose `name` is its own
  folder's, and carry a `description` (in any common YAML form, joined to one line, at most 1,024
  characters, Claude Code's own limit). The description goes through the filter Codex's index
  uses: any whitespace becomes one space, and every other control, separator or format character
  and every variation selector is removed, so none reaches Claude's frontmatter; one left empty refuses the pack. It is rendered as `name: <pack>--<skill>` and that
  description alone, as stetdoc's is, so `allowed-tools`, `hooks` and every other key go. Then the
  skills renderer's own checks (`skills::check`) run on it. One refused skill refuses the whole
  pack.

A refused pack has a `Refusal` code (`link`, `hard_link`, `bad_path`, `unlisted`, `too_big`,
`manifest`, `bad_id`, `id_mismatch`, `script`, `script_in_instruction_only`, `skill`, `missing`,
`not_a_file`, `io`). Log lines carry the id and the code, never a path or the pack's text (0047).

### The content hash

The hash is `sha256:` followed by 64 lower-case hex digits: the SHA-256 of one stream over every
file in the pack, the manifest included (`packs::hash`, format `clauseo-pack-hash-v1`):

```text
"clauseo-pack-hash-v1" 0x00
for each file, sorted by its path relative to the pack (/-separated, compared as bytes):
    the path's length in bytes, as a little-endian u64, then the path
    the file's length in bytes, as a little-endian u64, then its bytes
```

Every field carries its length, so a NUL inside a file cannot stand for a file boundary and two
different packs never feed the hasher the same stream (`review_hash_collision_swaps_an_approved_pack`).
The domain tag keeps a pack's hash apart from any other SHA-256 Clauseo takes. This is not 0031's
`versions.lock` algorithm (`path 0x00 bytes 0x00`, no lengths), which covers only Clauseo's own
compiled-in files, is no trust gate, and stays as it is.

- **Turning a pack on** records its hash at that moment in `skills.enabled`, as a map from id to
  hash: `{"nda-kit": "sha256:…"}`.
- **At each launch** the hash is computed again from the bytes just read. When it differs, the pack
  is skipped as `Changed`. Nothing rewrites the setting; the stale hash is the disabled state.
  Turning the pack on again records the new hash.
- Built-ins are exempt: compiled in, and pinned by `versions.lock`.
- The workspace's `sha2` 0.10 uses the CPU's SHA instructions when it finds them at run time
  (SHA-NI on x86-64, through `cpufeatures`); iris has them.

### Each launch's own copy

There is no cache on disk. Each launch reads, checks, hashes and renders its packs in memory
(`packs::load`, one thread per pack), then writes them into its own plugin folder from those
same bytes (`packs::write_packs`, one thread per pack). Each file is a new file with the default
mode, never an exec bit, and is not synced: the folder is disposable and goes with the run
folder. No file is shared between launches, so a write through one launch's copy (an agent, or
anything else that can write the run folder) reaches no other session, running or later, and the
next launch writes the pack's checked bytes again. Integrity does not rely on the jail.

`packs::load` returns the loaded packs and the skipped ones with a `Skip` reason
(`Refused(code)`, `Changed`, `TooMany`).

### Settings

A new `skills` section (PR-B). `skills.extra`, `skills.allow_scripts` and `skills.curated` are
security keys (`leaf::SECURITY_KEYS`). A policy source that cannot be read locks them at `off`,
`false` and the empty list. A malformed policy value does the same, as for `tools.*`.

| Key | Values | Default | Who sets it |
|---|---|---|---|
| `skills.extra` | `off`, `curated`, `any` | `off` | The lawyer; a policy value locks it |
| `skills.curated` | list of pack ids | empty | **Policy only**: a `settings.json` value is ignored with a warning |
| `skills.allow_scripts` | boolean | `false` | The lawyer; a policy value locks it |
| `skills.enabled` | map from pack id to hash | empty | The lawyer, through `SetSkillPack`; a policy value locks it |

`skills.curated` being policy-only needs a small mechanism in `settings` (PR-B): a list of
policy-only keys whose `settings.json` value is ignored with a warning, beside the off-only keys.
`off` loads no extra pack. `curated` loads only ids on `skills.curated`; without a policy that list
is empty, so `curated` admits nothing. `any` loads every pack that passes validation. Built-ins
load whatever these say. An entry of `skills.enabled` with a bad id or hash is dropped with a
warning.

### Per matter

Matter details gain `skill_packs: Option<Vec<PackId>>`, persisted as H5 persists `client` and
`next_date`. In `UpdateMatter` it is a `FieldChange`: missing leaves it, `null` clears it, and a
list sets it. The list holds at most 32 ids with no duplicates; an empty list is valid. It is
stored in a new column (store migration `0005`, JSON text) and returned on `MatterSummary`.

- Missing means the matter uses `skills.enabled`. An empty list means no extra pack for this
  matter.
- **A matter's list picks from the packs turned on in `skills.enabled`.** It carries no hashes. An
  id that is not turned on is skipped as `NotTurnedOn`. So a matter can narrow the global list, not
  widen it, and turning a pack on (with its hash) stays the one trust step.
- **Forgetting a matter keeps its list, as H5 keeps `client` and `next_date`**, and unforget
  restores it with the other details. Restoring it cannot bring back an untrusted pack: the list
  only narrows the packs turned on in `skills.enabled` whose recorded hash still matches the bytes
  on disk, so a pack turned off, or changed, since the forget stays out.
- No UI in this work. The app thread reviews the `bindings.ts` change.

Per matter, as built (PR-C): the wire types are `PackId` (a checked id) and `PackList` (at most 32
ids, no duplicates, checked on decode), both in `protocol`. The store keeps the list in a new
column (migration `0005_matter_skill_packs.sql`, JSON text, NULL for every matter before it).
Forget keeps the list and unforget restores it as it was. Narrowing to `skills.enabled` and the
hash check are left to the launch (PR-D).

### Loading, rendering and the launch

- **On the actor:** the launch's `PackRequest`: the ids that apply (the matter's list, or
  `skills.enabled`), each with its recorded hash, plus the effective `extra`, `curated` and
  `allow_scripts`. It is pure data, and no pack file is read on the actor.
- **On the launch-prep thread,** after the built-in skills are written: `packs::load` (read, check,
  hash against the record, render in memory), then `packs::write_packs` into **a plugin of the
  launch's own**, `run/<session id>/skill-packs/`, created fresh. That folder holds
  `.claude-plugin/plugin.json` (named `clauseo-packs`) and `skills/<pack>--<skill>/`, each file
  this launch's own copy, written from memory. It must not exist before; the run folder is removed on every way out, so a
  pack that was turned off or skipped is never left loading, and two matters running at once each
  see only their own packs. A failed write removes the folder, is a warning, and the launch goes on
  with the built-ins.
- `write_packs` returns a `PacksWritten` token (private fields, made only there) with the skills it
  wrote.
- **Claude** gets a second `plugins` entry naming the launch's pack plugin
  (`ClauseoProfile::session_meta_with_packs`), only with a token that wrote a skill.
- **Codex** gets one index (`ClauseoProfile::codex_keys_with_packs`): the built-in lines first,
  exactly as 0031 renders them (only with `SkillsWritten`), then one line per pack skill (only with
  `PacksWritten`): `- <pack>--<skill> (<description>): <path>`. Each description is cut to 24 words
  and to 200 characters (at a character boundary, `…` included, so text with no whitespace is cut
  too), and filtered as 0031 filters paths (control and format characters, variation selectors,
  line and paragraph separators, host placeholders); a description that fails gives no use text.
  Lines stop at 40 entries, 600 words or 4,800 characters for the whole index, heading included;
  the built-in lines come first and are never cut, and the count of pack lines dropped is logged.

### Speed

Measured on iris with the ignored test `timing_load_and_write`
(`crates/profile/tests/packs/timing.rs`), release build, page cache warm, 15 launches each,
median / slowest. Targets are for the whole of `load` and `write_packs`:

| Case | `load` | `write_packs` | Launch | Target |
|---|---|---|---|---|
| 2 packs, 3 skills each, 29 KiB | 0.43–0.46 / 0.64–0.75 ms | 0.74–0.75 / 0.86–1.0 ms | 1.2 / 1.4–1.6 ms | 50 ms |
| 16 packs at the caps, 253 files and 7.9 MiB each, 125 MiB | 61–65 / 182–216 ms | 52–53 / 57–61 ms | 115–119 / 237–271 ms | 200 ms |

(Three runs, load average about 1.) At the caps the median launch meets the target, but the slowest
does not: it is the first of the 15 launches, right after the packs were written, where `load`
alone takes about 200 to 300 ms; the later launches stay near the median. No per-launch total cap
is added; that is a decision for the host. A debug build is far slower at the caps, since the
workspace's dev profile compiles dependencies, `sha2` among them, without optimisation.

### Turning packs on, and reporting a skipped pack

- `HostCommand::SetSkillPack { id, on }` (PR-E). Turning on checks the policy on the actor, then
  reads, checks and hashes the pack off the actor (`packs::inspect`), then writes `skills.enabled`
  on the actor, as `SetToolSettings` writes. It is refused with a code when policy does not admit
  the pack, the pack is invalid, or `skills.enabled` is locked. Turning off removes the entry.
- `Query::ListSkillPacks` (PR-E) gives each installed pack's manifest fields, its current hash and
  its state: `On`, `Off`, `Changed`, `NotAdmitted` or `Refused(code)`. The later UI reads it.
- **A skipped pack:** a host log line holding the id and the reason code, with no path or pack text
  (0047). Plus one `Notice` warning in the thread per launch that skipped any packs, naming each by
  id: "Skill pack nda-kit was not loaded: it changed since you turned it on." The thread view
  already shows notices.

### Windows

Beyond the rules above: any reparse point is refused, the link count and final path are read from
the open handle, and the 160-byte relative-path cap keeps rendered paths short. A data folder too
long for `MAX_PATH` gets a failed write: a warning, with the packs left out. Claude and Codex read
the rendered paths themselves, so Clauseo does not rely on `\\?\` long paths.

### Budgets

0031's table is amended: Codex's skills index is at most 150 words for the built-ins, and at most
600 words, 4,800 characters and 40 entries in total, so Codex's `developer_instructions` can reach about 2,050 words.
The 1,500-word test still covers the text both agents share, and 0031's index test still covers the
built-ins alone.

### The code

- `profile` (PR-A, with this record): `packs` (`manifest`, `read`, `windows`, `scripts`, `render`,
  `hash`, `plugin`, `index`), `ProfilePaths::skill_packs`,
  `ClaudeOptions::with_pack_plugin`, `ClauseoProfile::session_meta_with_packs`,
  and `codex_keys_with_packs`.
- `settings` (PR-B): the `skills` section and keys, the policy-only keys, and
  `Effective::set_skill_pack`.
- `protocol`, `store` and `host` (PR-C): `PackId`, the `skill_packs` field, migration `0005`, and
  the forget and unforget tests that show the list is kept.
- `host` and `desktop` (PR-D): `PackRequest`, `ProfileHook::pack_write`, `load` and `write_packs`
  on the prep thread, the plugin and index wiring, the log line and
  the notice.
- `protocol`, `host` and `profile` (PR-E): `SetSkillPack`, `ListSkillPacks` and the refusal codes.

## Consequences

- A Hub needs no new loading code. It installs folders under `<data>/skill-packs/` and, for firms,
  a policy with `skills.extra: curated` and a `skills.curated` list.
- Clauseo never runs a pack's files. With `skills.allow_scripts` true, the agent can still run what
  a pack ships, through its own shell and under its jail. That setting is the firm's or the
  lawyer's explicit choice, off by default and off whenever policy cannot be read.
- A pack's descriptions reach Codex's `developer_instructions` and Claude's skill list. Turning a
  pack on, and policy, are the gate. The untrusted-content rule (0031) covers what the agent reads
  in a pack's files.
- Each launch writes its own copy of its packs. Stage C should let each jail read its own
  `run/<session id>/skill-packs/`; a write there changes only that launch's copy, so the packs'
  integrity does not depend on the jail. At the caps a launch writes about 125 MiB to the page
  cache.
- A change to a pack's files after it was turned on keeps it out of every launch until the lawyer
  turns it on again.
- The pack format is `format: 1` and the hash `clauseo-pack-hash-v1`. A change to the manifest,
  the hash or the layout needs its own record.

## Status

proposed

## Date

2026-11-05
