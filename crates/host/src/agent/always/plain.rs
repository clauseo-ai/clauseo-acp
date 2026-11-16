//! Whether a command a card shows is **one plain command**, the only kind "Always for this
//! matter" keeps (decision record 0063). Pure text work: no file system, no clock.
//!
//! - The text is read the way the shell that runs it would read it, by a grammar much smaller
//!   than the shell's ([`Grammar`]): anything it does not fully understand fails closed.
//! - The program must not run other code it names or reads ([`WRAPPERS`], [`refused_args`]).
//! - No argument may name a place outside the matter by its text ([`outside`]): an absolute path
//!   outside the matter folder, or one that leaves it with `..`.
//! - An agent's own shell wrapper is read through once, for Codex only ([`Agent::Codex`]):
//!   codex-acp 2.2.2 shows Codex's `[zsh, -lc, script]` as
//!   `stripShellPrefix(shlex_join(argv))`, usually the script alone, and the
//!   `bash|sh|zsh -c|-lc <script>` form this leaves for any other shell path is read through to
//!   its script.

/// The longest command kept, in bytes.
pub(crate) const MAX_COMMAND_BYTES: usize = 8 * 1024;

/// Which shell's words the command is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Grammar {
    /// A POSIX shell (Claude's `Bash`, Codex's `zsh -lc` on macOS and Linux).
    Posix,
    /// PowerShell (Claude's `PowerShell`, Codex's `powershell -Command` on Windows).
    PowerShell,
}

/// Whose card it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Agent {
    /// Claude Code: its shell tools take the script as a string.
    Claude,
    /// Codex: codex-acp shows its command line, a shell wrapper usually stripped.
    Codex,
}

/// Where the command runs, for [`outside`]: the matter folder and the working folder, as text.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Place<'a> {
    pub(crate) matter: &'a str,
    pub(crate) cwd: &'a str,
    /// Windows paths (drive letters, `\`, case folded).
    pub(crate) windows: bool,
}

/// Why a command is not one plain command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NotPlain {
    /// Empty, or longer than [`MAX_COMMAND_BYTES`].
    Size,
    /// A character or a form the grammar does not take: a chain, a pipe, a redirection, a
    /// substitution, a glob, a variable, an escape, an unclosed quote, an invisible character.
    Unparsed,
    /// A shell, an interpreter, a program runner, or a program given another program to run.
    Wrapper,
    /// An argument that names a place outside the matter.
    Outside,
}

/// The one plain command `line` runs, as its words, or why it has none.
pub(crate) fn plain(
    line: &str,
    grammar: Grammar,
    agent: Agent,
    place: Place<'_>,
) -> Result<Vec<String>, NotPlain> {
    if line.is_empty() || line.len() > MAX_COMMAND_BYTES {
        return Err(NotPlain::Size);
    }
    let mut words = split(line, grammar)?;
    if agent == Agent::Codex
        && let Some((script, inner)) = own_wrapper(&words)
    {
        words = split(&script, inner)?;
        return checked(words, inner, place);
    }
    checked(words, grammar, place)
}

fn checked(
    words: Vec<String>,
    grammar: Grammar,
    place: Place<'_>,
) -> Result<Vec<String>, NotPlain> {
    let (program, args) = words.split_first().ok_or(NotPlain::Unparsed)?;
    if !program_name(program, grammar) {
        return Err(NotPlain::Unparsed);
    }
    if wrapper(program) || refused_args(program, args) {
        return Err(NotPlain::Wrapper);
    }
    if args.iter().any(|word| outside(word, place)) {
        return Err(NotPlain::Outside);
    }
    Ok(words)
}

/// The script of Codex's own wrapper, and the grammar it is in: `[sh-like, -c|-lc, script]`
/// with a shell of [`POSIX_SHELLS`] (any folder), or `[powershell|pwsh, flags…, -Command|-c,
/// script]` with only `-NoProfile`, `-NoLogo` or `-NonInteractive` as flags.
fn own_wrapper(words: &[String]) -> Option<(String, Grammar)> {
    let program = base_name(words.first()?);
    if POSIX_SHELLS.contains(&program.as_str()) {
        let [_, flag, script] = words else {
            return None;
        };
        return matches!(flag.as_str(), "-c" | "-lc").then(|| (script.clone(), Grammar::Posix));
    }
    if matches!(program.as_str(), "powershell" | "pwsh") {
        let (script, rest) = words.get(1..)?.split_last()?;
        let (command, flags) = rest.split_last()?;
        let command =
            command.eq_ignore_ascii_case("-command") || command.eq_ignore_ascii_case("-c");
        let flags_ok = flags.iter().all(|flag| {
            ["-noprofile", "-nologo", "-noninteractive"]
                .iter()
                .any(|known| flag.eq_ignore_ascii_case(known))
        });
        return (command && flags_ok).then(|| (script.clone(), Grammar::PowerShell));
    }
    None
}

/// The shells whose `-c` script Codex's wrapper is read through to.
const POSIX_SHELLS: [&str; 3] = ["bash", "sh", "zsh"];

/// `line`'s words under `grammar`, or [`NotPlain::Unparsed`].
fn split(line: &str, grammar: Grammar) -> Result<Vec<String>, NotPlain> {
    let mut words = Vec::new();
    let mut word: Option<String> = None;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c == ' ' || c == '\t' {
            words.extend(word.take());
            continue;
        }
        // PowerShell reads `''` inside quotes as one quote, and joins quoted and bare text by
        // rules of its own: a quote there opens a word and ends it.
        let whole = grammar == Grammar::Posix || word.is_none();
        let current = word.get_or_insert_with(String::new);
        match c {
            '\'' if whole => {
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(inner) if quoted_ok(inner, '\'') => current.push(inner),
                        _ => return Err(NotPlain::Unparsed),
                    }
                }
                let ends = chars.peek().is_none_or(|next| matches!(next, ' ' | '\t'));
                if grammar == Grammar::PowerShell && !ends {
                    return Err(NotPlain::Unparsed);
                }
            }
            '"' if grammar == Grammar::Posix => loop {
                match chars.next() {
                    Some('"') => break,
                    Some(inner) if quoted_ok(inner, '"') => current.push(inner),
                    _ => return Err(NotPlain::Unparsed),
                }
            },
            c if bare_ok(c, grammar) => current.push(c),
            _ => return Err(NotPlain::Unparsed),
        }
    }
    words.extend(word);
    if words.is_empty() {
        return Err(NotPlain::Unparsed);
    }
    Ok(words)
}

/// A character taken outside quotes: ASCII letters and digits, a few marks, and any other
/// letter or digit. A POSIX shell would read none of these as more than itself.
fn bare_ok(c: char, grammar: Grammar) -> bool {
    match grammar {
        Grammar::Posix => {
            c.is_ascii_alphanumeric()
                || matches!(c, '_' | '-' | '.' | '/' | ':' | ',' | '+' | '=' | '@' | '%')
                || (!c.is_ascii() && c.is_alphanumeric())
        }
        Grammar::PowerShell => {
            c.is_ascii_alphanumeric()
                || matches!(c, '_' | '-' | '.' | '/' | '\\' | ':' | ',' | '+' | '=')
                || (!c.is_ascii() && c.is_alphanumeric())
        }
    }
}

/// A character taken inside `quote` quotes: none that a POSIX shell expands there, no control
/// character, and nothing invisible.
fn quoted_ok(c: char, quote: char) -> bool {
    if c.is_control() || invisible(c) {
        return false;
    }
    match quote {
        '"' => !matches!(c, '"' | '$' | '`' | '\\' | '!'),
        _ => c != '\'',
    }
}

/// A character that shows as nothing or turns text: format characters, line and paragraph
/// separators, and blank spaces other than the ASCII one.
fn invisible(c: char) -> bool {
    matches!(c,
        '\u{00ad}' | '\u{061c}' | '\u{180e}' | '\u{200b}'..='\u{200f}' | '\u{2028}'..='\u{202e}'
        | '\u{2060}'..='\u{206f}' | '\u{feff}' | '\u{fff9}'..='\u{fffb}'
    ) || (c.is_whitespace() && c != ' ')
}

/// A program's name as typed: ASCII letters, digits, `.`, `_`, `+` and `-`, not starting with
/// `-`; no folder (a program in the matter is a file the agent can write) and no `=` (a
/// variable set for the command).
fn program_name(program: &str, _grammar: Grammar) -> bool {
    !program.starts_with('-')
        && program
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '+' | '-'))
}

/// `program` without its folder, lower case, without `.exe` and the like and without a version
/// at its end (`python3.12` is `python`).
fn base_name(program: &str) -> String {
    let name = program
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(program)
        .to_ascii_lowercase();
    let name = [".exe", ".cmd", ".bat", ".com", ".ps1"]
        .iter()
        .find_map(|suffix| name.strip_suffix(suffix))
        .unwrap_or(&name);
    name.trim_end_matches(|c: char| c.is_ascii_digit() || c == '.' || c == '-')
        .to_owned()
}

/// Whether `program` is on [`WRAPPERS`], by its name as typed or its [`base_name`].
fn wrapper(program: &str) -> bool {
    let typed = program.to_ascii_lowercase();
    let base = base_name(program);
    WRAPPERS
        .iter()
        .any(|listed| *listed == typed || *listed == base)
}

/// Programs never kept: each runs other code that its words name or a file holds, reaches the
/// network, changes what later commands do, or opens something interactive. Lower case.
#[rustfmt::skip]
pub(crate) const WRAPPERS: &[&str] = &[
    // Shells.
    "sh", "bash", "zsh", "dash", "ksh", "mksh", "yash", "fish", "csh", "tcsh", "ash", "busybox",
    "nu", "xonsh", "elvish", "rc", "pwsh", "powershell", "powershell_ise", "cmd", "wsl",
    // Interpreters and compilers that run what they compile.
    "python", "py", "pythonw", "ipython", "jupyter", "pypy", "node", "nodejs", "deno", "bun",
    "ts-node", "tsx", "ruby", "irb", "erb", "perl", "php", "lua", "luajit", "r", "rscript", "julia",
    "tclsh", "wish", "expect", "osascript", "swift", "java", "jshell", "kotlin", "kotlinc", "scala",
    "groovy", "clojure", "elixir", "erl", "escript", "iex", "ghci", "runghc", "racket", "guile",
    "sbcl", "ocaml", "awk", "gawk", "mawk", "nawk", "sed", "m4", "bc", "dc", "sqlite", "cscript",
    "wscript", "mshta", "dotnet", "csi", "go",
    // Program runners.
    "env", "sudo", "doas", "su", "runas", "pkexec", "xargs", "nohup", "time", "nice", "renice",
    "ionice", "timeout", "gtimeout", "stdbuf", "unbuffer", "chroot", "chrt", "taskset", "flock",
    "caffeinate", "script", "watch", "parallel", "exec", "command", "builtin", "eval", "source",
    ".", "strace", "ltrace", "dtrace", "dtruss", "gdb", "lldb", "valgrind", "open", "xdg-open",
    "start", "explorer", "launchctl", "automator", "shortcuts", "at", "batch", "crontab",
    "systemd-run", "systemctl", "service", "schtasks", "sc", "reg", "wmic", "rundll32", "regsvr32",
    "msiexec", "installer", "security", "screen", "tmux",
    // Build and package tools: they run scripts the folder holds.
    "make", "gmake", "cmake", "ninja", "meson", "bazel", "just", "rake", "npm", "npx", "pnpm",
    "pnpx", "yarn", "yarnpkg", "bunx", "corepack", "cargo", "rustup", "gradle", "gradlew", "mvn",
    "mvnw", "ant", "sbt", "bundle", "bundler", "gem", "pip", "pipx", "uv", "uvx", "poetry", "pdm",
    "conda", "mamba", "micromamba", "tox", "nox", "pytest", "py.test", "jest", "vitest", "mocha",
    "composer", "brew", "port", "apt", "apt-get", "dpkg", "dnf", "yum", "pacman", "zypper", "snap",
    "flatpak", "winget", "choco", "scoop", "nuget", "msbuild", "xcodebuild",
    // Version control: its config and hooks run programs.
    "git", "hg", "svn",
    // The network.
    "curl", "wget", "nc", "ncat", "netcat", "socat", "telnet", "ftp", "sftp", "scp", "ssh", "rsync",
    "aria2c", "http", "https", "httpie", "lynx", "links", "w3m", "ping", "dig", "nslookup", "host",
    "whois", "traceroute", "certutil", "bitsadmin",
    // Editors and pagers: each can run a command.
    "vi", "vim", "nvim", "view", "ex", "ed", "nano", "pico", "emacs", "less", "more", "most", "man",
    // Shell state: what later commands run, or where.
    "cd", "pushd", "popd", "export", "set", "unset", "alias", "unalias", "trap", "shopt", "setopt",
    "ulimit", "umask", "exit", "logout", "hash",
    // PowerShell.
    "invoke-expression", "invoke-command", "icm", "start-process", "saps", "start-job",
    "invoke-item", "ii", "foreach-object", "where-object", "new-object", "add-type", "set-alias",
    "sal", "new-alias", "nal", "import-module", "ipmo", "invoke-webrequest", "iwr",
    "invoke-restmethod", "irm", "start-bitstransfer", "register-scheduledtask",
    "set-executionpolicy", "set-location", "sl", "chdir", "push-location", "pop-location",
    "new-pssession", "enter-pssession", "invoke-wmimethod", "invoke-cimmethod", "start-service",
    "restart-service", "set-variable", "sv", "set-item", "si", "test-netconnection",
];

/// Arguments that make an ordinary program run another, or write where its words don't say:
/// `find`'s actions and `tar`'s programs.
fn refused_args(program: &str, args: &[String]) -> bool {
    let refused: &[&str] = match base_name(program).as_str() {
        "find" => &[
            "-exec", "-execdir", "-ok", "-okdir", "-delete", "-fprint", "-fprint0", "-fprintf",
            "-fls",
        ],
        "tar" | "bsdtar" | "gtar" => &[
            "--to-command",
            "--use-compress-program",
            "-I",
            "--checkpoint-action",
            "--info-script",
            "-F",
            "--new-volume-script",
            "--rsh-command",
            "--rmt-command",
        ],
        _ => return false,
    };
    args.iter().any(|arg| {
        refused
            .iter()
            .any(|flag| arg == flag || arg.starts_with(&format!("{flag}=")))
    })
}

/// Whether `word` names a place outside the matter by its text: an absolute path (the word, or
/// what follows its first `=`) not inside the matter folder, or a relative one with a `..` that
/// leads out of it from the working folder. Compared as text, case folded on Windows.
fn outside(word: &str, place: Place<'_>) -> bool {
    let after = word.split_once('=').map(|(_, value)| value);
    [Some(word), after].into_iter().flatten().any(|path| {
        if absolute(path, place.windows) {
            !inside(path, place)
        } else if has_parent(path) {
            let joined = format!("{}/{path}", place.cwd);
            !inside(&joined, place)
        } else {
            false
        }
    })
}

fn absolute(path: &str, windows: bool) -> bool {
    if path.starts_with('/') {
        return true;
    }
    if !windows {
        return false;
    }
    let bytes = path.as_bytes();
    path.starts_with('\\') || matches!(bytes, [drive, b':', ..] if drive.is_ascii_alphabetic())
}

fn has_parent(path: &str) -> bool {
    path.split(['/', '\\']).any(|name| name == "..")
}

/// Whether `path`, made plain (`.` dropped, `..` taking its folder away), is the matter folder
/// or inside it.
fn inside(path: &str, place: Place<'_>) -> bool {
    let (Some(path), Some(root)) = (
        normal(path, place.windows),
        normal(place.matter, place.windows),
    ) else {
        return false;
    };
    path.strip_prefix(&root)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
        || root == "/"
}

fn normal(path: &str, windows: bool) -> Option<String> {
    let path = if windows {
        path.replace('\\', "/").to_lowercase()
    } else {
        path.to_owned()
    };
    let mut parts: Vec<&str> = Vec::new();
    for name in path.split('/') {
        match name {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            name => parts.push(name),
        }
    }
    let lead = if path.starts_with("//") {
        "//"
    } else if path.starts_with('/') {
        "/"
    } else {
        ""
    };
    Some(format!("{lead}{}", parts.join("/")))
}

#[cfg(test)]
#[path = "plain_tests.rs"]
mod tests;
