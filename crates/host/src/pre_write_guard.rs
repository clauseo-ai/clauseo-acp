//! The pre-write guard: the command an agent's `PreToolUse` hook runs before each file edit and
//! shell command, refusing a write to a document of the matter that Word (or, on Windows, any
//! program) holds open, in every permission mode.
//!
//! [`run`] reads one hook input object (Claude Code's or Codex's) and finds the files the tool
//! would write:
//!
//! - Claude Code's `Edit`, `Write`, `MultiEdit` and `NotebookEdit`: `tool_input.file_path` or
//!   `tool_input.notebook_path`.
//! - Codex's `apply_patch` (hooked under that name or its aliases `Write` and `Edit`): the
//!   `*** Add File:`, `*** Update File:`, `*** Delete File:` and `*** Move to:` lines of the
//!   patch in `tool_input.command`.
//! - `Bash` (both agents) and Claude Code's `PowerShell`, in two layers:
//!   - Paths, a heuristic (`words.rs`, `shell.rs`). `tool_input.command` is split into words
//!     the simple shell way (whitespace, quotes, the escape character, a backslash or
//!     PowerShell's backtick, escaped newlines, `$'…'` with bash's escapes; `#` comments and
//!     PowerShell's `<# … #>` dropped; no expansion), and into simple commands at each unquoted
//!     newline, `;`, `&`, `&&`, `|`, `||`, `(` and `)`, a heredoc's body read as data. The first
//!     [`MAX_WORDS`] words are kept, plus as many of each script handed to `sh -c`, `bash -c`,
//!     `eval`, `pwsh -Command` and the like (one level). A word names a target when it is an
//!     existing file, also once its shell punctuation is off (`candidates`). It is taken from the
//!     input's `cwd`, from every folder the command's `cd`, `pushd` or `Set-Location`
//!     (PowerShell's `cd..` and `cd\` too) may have reached by then, whether or not that change
//!     ran (`false && cd x`, a subshell), past prefixes such as `builtin`, `then`, `{` and
//!     `NAME=value`, by the folder's text and through symlinks, at most 16 folders; and from the
//!     folder it is in if every change ran in order, followed whatever the cap. A change it cannot
//!     follow (`cd` alone, `cd ~`, `cd -`, `pushd +1`, `cd "$(…)"`) makes the folder
//!     "unknown": the set stops growing past it and the in-order folder is lost until an absolute
//!     change; nothing else changes, the layers below still apply. When the command names a
//!     folder change anywhere, a relative word is also taken from `cwd` with its leading `..`
//!     parts off (a rare false refusal when a write elsewhere has a held file's name, accepted).
//!     PowerShell's `\` separates folders on every OS. Claude Code's `PowerShell` command, and a
//!     `Bash` one under Codex's hook on Windows (Codex's shell there is PowerShell; such a
//!     command is read the Bash way too, a refusal by either standing), is read with
//!     PowerShell's quoting (`about_Quoting_Rules`: `'O''Brien'`, `"a ""b"""`, the typographic
//!     quotes, the backtick; `words/powershell.rs`). A patch in the command (Codex's
//!     `exec_command` applies `apply_patch <<'EOF' …`) counts, its lines taken from `cwd`.
//!   - Names, a backstop that does not depend on following folders (`names.rs`). The files of
//!     the matter held open right now are listed (the folders with an Office owner file `~$…`,
//!     breadth first, at most 20 000 entries, no hidden folder, no symlink followed), and a
//!     command that holds one's name as a whole name (between the start, a folder separator,
//!     whitespace, a quote or a shell metacharacter, and the end, whitespace, a quote or a
//!     metacharacter; ignoring case on macOS and Windows, and Unicode form everywhere), in its
//!     text (escaped newlines joined, `$'…'` decoded, comments kept) or in its words as the shell
//!     reads them (quotes and escapes off: `Con""tract.docx`, `My\ Deal.docx`; and one level of
//!     `sh -c` scripts), or has a word whose wildcard (`*`, `?`, `[…]`, in its last part) could
//!     match one's name in any folder (`cp Revised/*.docx .`, `for f in *.docx`), is refused,
//!     as is one whose words name a held file by path (a symlink, another case or form), unless
//!     the whole command only reads. Read-only is narrow and whole-command, and when in doubt it
//!     is not (`names::read_only`): no `$(`, backtick, `<(`, `>(`, `$_` or function definition
//!     (no `{` under PowerShell); every simple command, by a bare program name (no `/`, no
//!     `NAME=value` before it), is a reader, which may name the held file (`cat`, `head`,
//!     `tail`, `wc`, `strings`, `ls`, `stat`, `test`/`[`, checksums, `zipinfo`, `diff`,
//!     `cmp`, PowerShell's `Get-Content`/`type`/`Select-String`/`Test-Path`, `file`, `unzip`
//!     listing, or extracting with `-d` outside the matter, `textutil` and `pandoc` whose
//!     outputs name no held file and with no option that runs code, `cp` of one file to another
//!     file naming no held file or into a folder outside the matter, a `stetdoc` form writing
//!     no held file, `sh -c` whose script passes and whose other words name no held file), or
//!     a filter (`grep`, `rg`, `sed`
//!     without `-i`, `sort` without `-o`, `uniq`, `cut`, `tr`, `nl`, `column`) or neutral
//!     command (`cd`, `pushd`, `popd`, `echo`, `pwd`, `true`), which must not name it; and
//!     each output redirection goes to `/dev/null`, `$null`, a descriptor (`2>&1`, `>&2`,
//!     `>&-`) or a literal path naming no held file (never `>&file`, a `$`, a glob, a brace or
//!     `~`). So while a document is open in Word, `cat Contract.docx | grep x > notes.txt`,
//!     `cp Contract.docx copy.docx` and `pandoc Contract.docx -o summary.md` go on, but `echo
//!     Contract.docx`, `grep x Contract.docx`, `less Contract.docx`, `cat Contract.docx; rm x`
//!     or `python -c` naming it is refused, with the held-open reason. Word opening a document
//!     through a short (8.3) folder path, which it does when the full path is over 259 units,
//!     creates no `~$` owner file, so this backstop cannot see such a document: it lists only the
//!     files beside an owner file. The path layer's sharing probe
//!     ([`clauseo_matters::held_open_at`], a no-share open that fails) still refuses it, and so
//!     does the OS's own sharing violation on any write. The agent's own Read tool is not checked.
//!   - stetdoc and textutil (`names::stetdoc`, read with stetdoc v0.9.1's options and their
//!     arity by `crate::stetdoc_args`, which the turn's shell writes share, so a flag's value
//!     is never taken for the document): `view`, `inspect`, `check`,
//!     `lookup`, `capabilities` and `skills` only read; an output (`-o`, `--output`, `-oX`,
//!     `--output=X`) naming a held file writes it; with `--in-place` any word naming a held file
//!     is written; else `--dry-run` writes nothing; else `edit`, `replace` and `comment` write
//!     `NAME.edited.docx` beside the document and `pack` `NAME.edited.docx`. `textutil -convert
//!     FMT` writes each input's name with `FMT` (or `-extension`) as its extension, unless
//!     `-output` or `-stdout`. A form writing a held file is refused even when the command names
//!     none (a default output: `textutil -convert docx Contract.txt`), as are `stetdoc mcp` and
//!     `stetdoc init` outright; an unknown subcommand or option in a command naming a held file is
//!     refused.
//!
//!   Not caught: the guard stops an agent overwriting a held document by accident; it is not a
//!   boundary against an agent set on evading it (stage C's sandbox is). It misses a command
//!   whose writes never name the held file, by name, by a wildcard in a word, or by a path: brace
//!   expansion (`Con{tract,x}.docx`), a value built at run time (`f=Con; > $f"tract.docx"`),
//!   batch commands beyond that wildcard rule (`find … -exec` over `.`, `rsync -a Revised/ .`,
//!   `unzip -o archive.zip` whose contents land on the held file, `soffice --convert-to docx
//!   Contract.rtf`), a hard link to the held file, a glued short output through a symlink
//!   (stetdoc's or pandoc's `-oLink.docx`), a script file the command runs or a `sed`/`rg`
//!   script read from a file, a reader's own code paths (a program shadowing `cat` in a `PATH`
//!   set before the command, a shell profile); a file held on Windows by a share mode alone (no
//!   owner file) reached by a route the path layer misses; a held file past the 20 000 entries
//!   listed. The wildcard rule refuses on the safe side: a wildcard matching a held name refuses
//!   wherever the command writes inside the matter, quoted or not (`cp Revised/*.docx Archive/`
//!   too). Only a quoted pattern that a search matches itself is left alone (`find -name`,
//!   `-iname`, `-path` with no writing action, `rg -g`/`--glob`, `grep --include`/
//!   `--exclude`, PowerShell `Get-ChildItem -Filter`/`-Include`), when nothing in the command
//!   but readers of its output acts on what it finds. So, while a `.docx` is held, harmless
//!   commands over `*.docx` are refused: `echo *.docx`, a `for f in *.docx` loop even when its
//!   body only reads, `tar`/`zip` of `*.docx`, `git add *.docx`, `du -sh *`, `grep -l x *`,
//!   `python3 x.py *.docx`, `mkdir -p out && cp *.docx out/`. On Windows a folder outside the
//!   matter (for `cp` and `unzip -d`) is recognised only as a drive path (`C:/…`, or `C:\…` in
//!   quotes), not `/tmp` or Git Bash's `/c/…`, which are refused (the safe side). Under Codex, a
//!   `.codex` folder named by a glob (`cd .cod?`) is not caught. A command given as an argument
//!   list (a shape Codex used before) is judged read-only as its parts quoted and joined into one
//!   line.
//!
//! The matter folder is the `--matter` argument when the hook passes one (Claude Code's entry
//! does, `agent/loads.rs`: Claude Code's `cwd` follows the agent's `cd`), else the input's
//! `cwd` (Codex's hook: its `cwd` is the turn's working folder, which a command's `cd` does not
//! move). A relative target is taken from the input's `cwd`. Each target is checked twice with
//! [`clauseo_matters::held_open_at`] against the matter folder, the permission check's own rule
//! (both sides resolved canonically: symlinks, `..`, case on macOS and Windows; a target outside
//! the folder not checked): as spelled, and, when it has a `.` or `..` part, resolved through
//! its deepest existing folder both as spelled (a symlink's `..` is its target's parent) and
//! normalised by its text first (`a/../b` is `b`), so a `..` after a folder that does not exist
//! yet (`Drafts/../Contract.docx`, which `mkdir -p Drafts` would make good) is caught. A path
//! over 4096 bytes is only normalised by its text, so a huge word costs time in proportion to
//! its length. The first one held open is refused.
//!
//! One more rule for Codex's writes: a patch target in the matter's `.codex` folder (the folder
//! or anything under it; a move's source and destination both) is refused, held open or not.
//! codex-acp trusts the session folder, so a `.codex/config.toml` there loads, and in
//! workspace-write Codex could write one without asking (clauseo-acp-private#184). A target
//! counts when either resolution puts it there: canonical (its deepest existing folder
//! canonicalised, the rest added, then [`clauseo_matters::matter_path_in`], so a symlinked alias
//! of the matter, another case or a `\\?\` prefix is caught even before `.codex` exists) or
//! lexical (`..` takes a part off, so `sub/../.codex/x` is caught with no `sub` yet). The name
//! compares ignoring ASCII case and trailing dots and spaces, as macOS and Windows file systems
//! do. In a shell command, where the working folder is unknown, any patch target with a `.codex`
//! part is refused, and under Codex's hook (the one with no `--matter` argument) so is a `Bash`
//! command any of whose words, once its shell punctuation is off, has a `.codex` part, relative
//! or absolute: `touch .codex/config.toml`, `echo x > .codex/config.toml`, and reads such as
//! `cat .codex/config.toml` too, since an agent has no reason to touch a folder Clauseo refuses
//! it. Claude Code needs no such rule: its `settingSources` leave out project settings.
//!
//! And Codex's own home, when its hook names it with [`CODEX_HOME_FLAG`]: see `codex.rs`.
//!
//! One rule for Claude Code's writes: the paths its entry names with [`PROTECTED_FLAG`] (Claude's
//! own settings, instructions, skills and plugins in its config folder, the built-in plugin and
//! the run's packs plugin) are refused to its file tools, and to a shell command that names one
//! in either reading and does more than read, a reference to the config folder's variable read
//! as the folder ([`CLAUDE_HOME_FLAG`]), in a folder change too (`protected.rs`).
//!
//! A refusal is exit code 2 with the reason on stderr, which Claude Code and Codex both take as a
//! block, and, with [`RECORD_FLAG`] and a file and [`RECORD_ID_FLAG`] with the file's identity
//! (Codex's hook), one line in that file for the host to show the lawyer (`record.rs`); stdout
//! stays empty, so a login profile's output under Codex's `$SHELL -lc` cannot spoil it. Only Claude
//! Code's entry in forced ask (`--ask-all`, `ask.rs`) writes stdout: its `ask` decision for every
//! tool it does not refuse but the local reads. An unknown tool, or a known one with nothing held,
//! is exit 0 with nothing written.
//!
//! It fails open, a decision (clauseo-acp-private#162): input it cannot read (empty, cut short,
//! not JSON), or a shape it does not know (no absolute `cwd` or `--matter`, no `tool_name` or
//! `tool_input`, a checked tool without its path field), is exit 0 with one stderr line starting
//! [`IGNORED`] saying why, naming no input content. Stdin comes from the agent CLI, not the
//! model, so a model cannot make it malformed; and failing closed would refuse every matched
//! tool, stopping the agent outright, the day Claude Code or Codex changes its input shape. The
//! fixtures in `tests/fixtures/pre_write_guard` pin how the guard reads the shapes captured for
//! the agent versions their README names; CI alone does not notice a vendor change, so they are
//! captured again from the real tool whenever a pinned Claude Code or Codex version moves. In
//! default and plan mode the permission check (ADR 0021) still stops a write to a held file; in
//! acceptEdits and bypassPermissions this guard is the only stop. It never panics.

mod answers;
mod ask;
mod codex;
mod names;
mod protected;
mod reach;
pub(crate) mod record;
mod shell;
mod timings;
mod words;

use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use clauseo_protocol::MatterPath;
use serde_json::Value;

pub use answers::{IGNORED, UNCHECKED};
use answers::{Ignored, Refusal};
use codex::{
    codex_home_arg, has_codex_part, in_codex_folder, names_codex_home, patch_targets,
    reaches_codex_home,
};
use protected::Protected;
// The data folder fence reads a command's `sh -c` scripts as this guard does
// (`agent/data_fence/command.rs`).
use reach::{Reach, existing_resolved, lexical};
pub(crate) use shell::script as shell_script;
use shell::{shell_targets, walked_resolutions};
pub(crate) use words::{
    ARRAY_COMMA, Escape, Joint, Role, Segments, powershell_words, shell_segments,
};

/// The first argument that makes the desktop binary run [`run`] on its stdin and stderr and
/// exit, before anything else starts. The hook entry passes it (`agent/loads.rs`).
pub const FLAG: &str = "--pre-write-guard";

/// The argument after [`FLAG`] that names the matter folder, followed by the folder.
pub const MATTER_FLAG: &str = "--matter";

/// The argument that names Codex's home (`CODEX_HOME`), followed by the folder: Codex's hook
/// passes it (`agent/loads/codex.rs`), and a target in that folder is refused ([`run`]).
pub const CODEX_HOME_FLAG: &str = "--codex-home";

/// The argument that names a path the agent may not change (`protected.rs`), followed by it;
/// repeated for each. Claude Code's entry passes them (`agent/loads.rs`).
pub const PROTECTED_FLAG: &str = protected::FLAG;

/// The argument that names Claude's config folder, which a reference to its variable in a shell
/// word stands for (`protected.rs`), followed by it.
pub const CLAUDE_HOME_FLAG: &str = protected::HOME_FLAG;

/// The argument that names the file each refusal the lawyer is told about is recorded in
/// (`record.rs`), followed by the file.
pub const RECORD_FLAG: &str = record::FLAG;

/// The argument after [`RECORD_FLAG`] and its file that gives the file's identity, followed by
/// it: the guard writes only a file with that identity (`record.rs`).
pub const RECORD_ID_FLAG: &str = record::ID_FLAG;

/// Makes the session `session`'s record file under the data folder `data`, as the host's
/// launch prep does (`record.rs`, [`record::make`]): its folder of its own, create-new, then
/// the file with `O_EXCL`. The file and its identity, written for [`RECORD_ID_FLAG`]; an error
/// when anything is already at the folder's place.
pub fn make_record(data: &Path, session: &str) -> std::io::Result<(PathBuf, String)> {
    record::make(data, session).map(|(file, id)| (file, id.hex()))
}

/// How many of a shell command's words are looked at.
pub const MAX_WORDS: usize = 64;

/// The exit code that refuses the tool; the reason goes to stderr.
pub const DENY: i32 = 2;

/// The most input read: a hook object is a few kilobytes, a patch rarely more than a megabyte.
/// Anything longer does not parse and fails open.
const MAX_INPUT: u64 = 16 * 1024 * 1024;

/// The guard. `args` are the arguments after [`FLAG`] (`--matter <folder>`, or none, then
/// [`RECORD_FLAG`] and its file and [`RECORD_ID_FLAG`] and its identity, or none; Claude Code's
/// entry adds [`PROTECTED_FLAG`] and a path for each protected one and [`CLAUDE_HOME_FLAG`] and
/// its folder); one hook
/// input object comes from `input`. Returns the exit code: [`DENY`], with the reason and a newline
/// on `errors`, when a target is refused; 0 with an [`IGNORED`] line when the input could not be
/// read, but as Codex's hook (no `--matter`) [`DENY`] with [`UNCHECKED`] when
/// it is empty, unreadable or not JSON; else 0 with nothing written. With
/// `CLAUSEO_GUARD_TIMINGS` set in the environment (not empty, not `0`), one more line follows,
/// the phase timings (`timings.rs`).
/// With `--ask-all` after the folder (forced ask, `agent/forced_ask.rs`), a tool it does not
/// refuse that is not one of `ask::LOCAL_READS` gets Claude Code's `ask` decision on
/// `output` (`ask.rs`); otherwise `output` stays empty.
pub fn run(args: &[OsString], input: impl Read, output: impl Write, errors: impl Write) -> i32 {
    let on = timings::wanted(std::env::var_os(timings::ENV).as_deref());
    run_timed(args, input, (output, errors), on)
}

/// [`run`], with the phase timings ([`timings`]) on or off; when on, their line follows any
/// other line on `errors`.
fn run_timed(
    args: &[OsString],
    input: impl Read,
    (mut output, mut errors): (impl Write, impl Write),
    on: bool,
) -> i32 {
    timings::start(on);
    let mut bytes = Vec::new();
    let found = match input.take(MAX_INPUT).read_to_end(&mut bytes) {
        Ok(_) => {
            timings::mark("read");
            let home = codex_home_arg(args);
            refusal(&bytes, matter_arg(args).as_deref(), home.as_deref(), args)
        }
        Err(_) => Err(Ignored::Unreadable),
    };
    timings::mark("decide");
    let refused = matches!(found, Ok(Some(_)));
    let (line, code) = match found {
        Ok(None) => (None, 0),
        Ok(Some(refusal)) => {
            let claude = matter_arg(args).is_some();
            let target = record::target(args);
            let record = record::record_of(&refusal, &bytes, claude);
            if let Some(((file, id), record)) = target.zip(record) {
                record::append(&file, id, &record);
            }
            (Some(refusal.reason()), DENY)
        }
        Err(ignored) if as_codex_hook(args) && ignored.unread() => {
            (Some(UNCHECKED.to_owned()), DENY)
        }
        Err(ignored) => (Some(ignored.line()), 0),
    };
    if !refused && ask::wanted(args) && ask::asks(&bytes) {
        // As for `errors` below: when the agent stopped reading, nothing more can be done.
        drop(
            output
                .write_all(ask::decision().as_bytes())
                .and_then(|()| output.write_all(b"\n"))
                .and_then(|()| output.flush()),
        );
    }
    // Nothing more can be done when the agent stopped reading; the exit code still decides.
    for line in line.into_iter().chain(timings::finish()) {
        drop(
            errors
                .write_all(line.as_bytes())
                .and_then(|()| errors.write_all(b"\n"))
                .and_then(|()| errors.flush()),
        );
    }
    code
}

/// Whether the guard runs as Codex's hook: no `--matter` (Claude's hook always passes one,
/// `agent/loads.rs`, `guard_entry`), with a record or without. There input that
/// is empty, unreadable or not JSON refuses ([`UNCHECKED`], [`DENY`]).
fn as_codex_hook(args: &[OsString]) -> bool {
    matter_arg(args).is_none()
}

/// The folder after [`MATTER_FLAG`], when `args` start with it and it is absolute.
fn matter_arg(args: &[OsString]) -> Option<PathBuf> {
    match args {
        [flag, folder, ..] if flag == MATTER_FLAG => {
            Some(PathBuf::from(folder)).filter(|folder| folder.is_absolute())
        }
        _ => None,
    }
}

/// Why the hook input `bytes` is refused, if it is, with `matter` the folder from the
/// arguments: a target in Codex's home `codex_home` ([`reaches_codex_home`]) or, under Codex's
/// hook, a `Bash` command whose text names that home ([`names_codex_home`], with the home
/// argument or without), a patch target in
/// the matter's `.codex` folder, or under Codex's hook (no `matter`) a `Bash` word with a
/// `.codex` part, else the first target that lies in the matter folder and is held open; the
/// paths `args` protects first (`protected.rs`).
/// [`Ignored`] when the input cannot be read or has a shape the guard does not know.
fn refusal(
    bytes: &[u8],
    matter: Option<&Path>,
    codex_home: Option<&Path>,
    args: &[OsString],
) -> Result<Option<Refusal>, Ignored> {
    if bytes.iter().all(u8::is_ascii_whitespace) {
        return Err(Ignored::Empty);
    }
    let hook: Value = serde_json::from_slice(bytes).map_err(|_not_json| Ignored::NotJson)?;
    timings::mark("parse");
    let cwd = hook
        .get("cwd")
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .filter(|cwd| cwd.is_absolute());
    let folder = matter
        .map(Path::to_path_buf)
        .or_else(|| cwd.clone())
        .ok_or(Ignored::NoFolder)?;
    let base = cwd.unwrap_or_else(|| folder.clone());
    let reach = Reach::new(&folder);
    let protected = Protected::of(args, &reach);
    let tool = hook
        .get("tool_name")
        .and_then(Value::as_str)
        .ok_or(Ignored::NoTool)?;
    let input = hook.get("tool_input").ok_or(Ignored::NoTool)?;
    // The matter's held files, listed once for every reading, when a shell command needs them.
    let held = std::cell::OnceCell::new();
    let place = Place {
        folder: &folder,
        base: &base,
        reach: &reach,
        codex_bash: matter.is_none() && tool == "Bash",
        codex_home,
        held: &held,
        protected: &protected,
    };
    // Under Codex's hook on Windows a `Bash` command is read as PowerShell and as Bash: a
    // refusal by either refuses.
    let bash = bash_escape(matter.is_none(), cfg!(windows));
    let readings: &[Escape] = if tool == "Bash" && bash == Escape::Backtick {
        &[Escape::Backtick, Escape::Backslash]
    } else {
        &[bash]
    };
    for escape in readings {
        let Some(targets) = targets(tool, input, (&base, &reach, &protected), *escape)? else {
            return Ok(None);
        };
        timings::mark("targets");
        if let Some(refusal) = judge(&targets, &place) {
            return Ok(Some(refusal));
        }
    }
    Ok(None)
}

/// Where a tool's targets are judged: the matter `folder`, the `base` its relative paths start
/// from, the walk that resolves them, whether it is a `Bash` call under Codex's hook, Codex's
/// home when the hook named it, the matter's held files ([`names::held_files`]), listed on
/// first need, and the protected paths.
struct Place<'a> {
    folder: &'a Path,
    base: &'a Path,
    reach: &'a Reach,
    codex_bash: bool,
    codex_home: Option<&'a Path>,
    held: &'a std::cell::OnceCell<Vec<names::Held>>,
    /// The paths the agent may not change (`protected.rs`).
    protected: &'a Protected,
}

/// Why `targets` are refused at `place`, if they are ([`refusal`]).
fn judge(targets: &Targets, place: &Place<'_>) -> Option<Refusal> {
    if let Some(refusal) = protected::refusal(targets, place) {
        return Some(refusal);
    }
    let Place {
        folder,
        base,
        reach,
        codex_bash,
        codex_home,
        held,
        ..
    } = *place;
    if codex_home.is_some_and(|home| reaches_codex_home(targets, home, base, reach))
        || (codex_bash && names_codex_home(targets))
    {
        return Some(Refusal::CodexHome);
    }
    let into_codex = match targets {
        Targets::Unread => return Some(Refusal::Unread),
        Targets::Named(_) => false,
        Targets::Patch(paths) => paths
            .iter()
            .any(|path| in_codex_folder(folder, base, path, reach)),
        Targets::Shell {
            patch, names_codex, ..
        } => patch.iter().any(|path| has_codex_part(path)) || (*names_codex && codex_bash),
    };
    if into_codex {
        return Some(Refusal::CodexFolder);
    }
    let held_at = |paths: &[PathBuf]| {
        paths
            .iter()
            .flat_map(|path| walked_resolutions(&base.join(path), reach))
            .find_map(|path| clauseo_matters::held_open_resolved(folder, &path))
            .map(|(path, _)| path)
    };
    // A held file found by a tool's path, as the matter lists it (the walk keeps the agent's
    // spelling, [`names::as_listed`]).
    let listed = |held: Option<MatterPath>| held.map(|path| names::listed_spelling(folder, path));
    match targets {
        Targets::Unread => Some(Refusal::Unread),
        Targets::Named(paths) | Targets::Patch(paths) => {
            let held = listed(held_at(paths));
            timings::mark("resolve");
            held.map(Refusal::HeldOpen)
        }
        Targets::Shell {
            files,
            patch,
            text,
            line,
            escape,
            ..
        } => {
            if let Some(path) = listed(held_at(patch)) {
                timings::mark("resolve");
                return Some(Refusal::HeldOpen(path));
            }
            // The first held file the words name by path, and every word that names one.
            let mut found = None;
            let mut hits: Vec<String> = Vec::new();
            for (path, word) in files {
                if let Some(held) = held_at(std::slice::from_ref(path)) {
                    found.get_or_insert(held);
                    if !hits.contains(word) {
                        hits.push(word.clone());
                    }
                }
            }
            timings::mark("resolve");
            let held = held.get_or_init(|| names::held_files(folder)).clone();
            names::shell_refusal(text, line, *escape, folder, found, &hits, reach, held)
        }
    }
}

/// The paths a tool would write, as the agent spelled them, and where they came from.
enum Targets {
    /// A file tool's path.
    Named(Vec<PathBuf>),
    /// The files of a Codex `apply_patch` patch.
    Patch(Vec<PathBuf>),
    /// A shell command whose reading, or one of its scripts', was cut short
    /// (`words::Segments::cut`).
    Unread,
    /// A shell command: the existing files it names (each with the word naming it), the files of
    /// a patch in its text, and whether a word of it has a `.codex` part.
    Shell {
        files: Vec<(PathBuf, String)>,
        patch: Vec<PathBuf>,
        names_codex: bool,
        /// Every place a word may name, existing or not, as spelled from each folder the command
        /// may be in and as walked ([`shell::walked_resolutions`]).
        named: Vec<PathBuf>,
        /// The command's text (an argument list's parts one per line) and its shell's escape.
        text: String,
        /// The command as one shell line (an argument list's parts quoted, joined by spaces).
        line: String,
        escape: Escape,
        /// The words naming a protected path (`protected.rs`).
        protected: Vec<String>,
    },
}

/// The paths the tool `tool` with `input` would write, `None` for a tool the guard does not
/// check; a shell command's words are files when they exist, taken from `base`, a `Bash` one's
/// read with `bash`'s escape ([`bash_escape`]), and those naming one of `protected` noted. A
/// checked tool whose input lacks the field its paths come from is ignored.
fn targets(
    tool: &str,
    input: &Value,
    (base, reach, protected): (&Path, &Reach, &Protected),
    bash: Escape,
) -> Result<Option<Targets>, Ignored> {
    let text = |key: &str| input.get(key).and_then(Value::as_str);
    let targets = match tool {
        "Edit" | "Write" | "MultiEdit" | "NotebookEdit" | "apply_patch" => {
            if let Some(path) = text("file_path").or_else(|| text("notebook_path")) {
                Targets::Named(vec![PathBuf::from(path)])
            } else if let Some(patch) = text("command") {
                Targets::Patch(patch_targets(patch))
            } else {
                return Err(Ignored::Shape("file_path, notebook_path or command string"));
            }
        }
        "Bash" => shell_targets(input.get("command"), base, bash, (reach, protected))?,
        "PowerShell" => shell_targets(
            input.get("command"),
            base,
            Escape::Backtick,
            (reach, protected),
        )?,
        _ => return Ok(None),
    };
    Ok(Some(targets))
}

/// How a `Bash` tool's command is read: as `PowerShell` under Codex's hook (no `--matter`) on
/// Windows, else as Bash. Codex names every shell call `Bash` (codex-rs `HookToolName::bash`,
/// `tools/handlers/shell/shell_command.rs`, `unified_exec/exec_command.rs`) and sends the
/// model's script as it is, which on Windows its shell runs as
/// `powershell.exe -Command <script>`: a real Codex 0.162.1 hook input on Windows (VM, 16 Nov
/// 2026) was `tool_name` `Bash` with `Set-Content -LiteralPath 'notes.txt' -Value 'hello'`.
/// Claude Code's `Bash` (with `--matter`) is Git Bash on Windows, so it stays Bash.
fn bash_escape(codex: bool, windows: bool) -> Escape {
    if codex && windows {
        Escape::Backtick
    } else {
        Escape::Backslash
    }
}

#[cfg(test)]
mod tests;
