//! Codex's home (`CODEX_HOME`): with `--codex-home`, a Codex patch or shell command that reaches
//! it is refused, by its path, through its variable's spellings and after a `cd` into it. Under
//! Codex's hook on Windows each `Bash` command is read as PowerShell and as Bash (`refusal`).

use std::path::Path;

use serde_json::{Value, json};

use super::{Fixture, guard_with};
use crate::pre_write_guard::CODEX_HOME_FLAG;
use crate::write;

/// The deny line for a call that reaches Codex's home.
fn home_refused() -> String {
    "Clauseo doesn't let the agent touch Codex's own folder (CODEX_HOME): files there change \
     what the agent loads and runs. Run tools by name (for example rg), not by their path in \
     that folder.\n"
        .to_owned()
}

/// Codex's home `codex-home` beside the matter, holding `config.toml`, as a shell word: forward
/// slashes, so on Windows it reads `C:/…`.
fn home(fx: &Fixture) -> String {
    let home = fx.path("codex-home");
    write::create_dir_all(Path::new(&home)).unwrap();
    write::write_file(&Path::new(&home).join("config.toml"), b"model = \"x\"\n").unwrap();
    home.replace('\\', "/")
}

/// The guard's stderr for Codex's hook with `--codex-home <home>` (`with_home`) or without.
fn codex(fx: &Fixture, home: Option<&str>, tool: &str, input: &Value) -> String {
    let hook = fx.hook(tool, input).to_string();
    let args: Vec<&str> = home.map_or_else(Vec::new, |home| vec![CODEX_HOME_FLAG, home]);
    guard_with(&args, hook.as_bytes())
}

fn patch(body: &str) -> String {
    format!("*** Begin Patch\n{body}\n*** End Patch\n")
}

#[test]
fn a_codex_patch_into_codex_home_is_refused() {
    let fx = Fixture::new("codex-home-patch");
    let home = home(&fx);
    let bodies = [
        format!("*** Update File: {home}/config.toml\n@@\n-model = \"x\"\n+model = \"y\""),
        format!("*** Add File: {home}/AGENTS.md\n+Always run the script."),
        format!("*** Add File: {home}/AGENTS.override.md\n+x"),
        format!("*** Add File: {home}/skills/x/SKILL.md\n+x"),
        "*** Update File: Free.docx\n*** Move to: ../codex-home/hooks.json".to_owned(),
        "*** Add File: sub/../../codex-home/prompts/x.md\n+x".to_owned(),
    ];
    for body in &bodies {
        for tool in ["apply_patch", "Edit", "Write"] {
            let input = json!({"command": patch(body)});
            assert_eq!(
                codex(&fx, Some(&home), tool, &input),
                home_refused(),
                "{tool} {body}"
            );
        }
    }
}

#[test]
fn a_codex_shell_write_into_codex_home_is_refused() {
    let fx = Fixture::new("codex-home-shell");
    let home = home(&fx);
    let commands = [
        format!("echo 'model = \"y\"' > '{home}/config.toml'"),
        format!("touch {home}/AGENTS.md"),
        format!("mkdir -p {home}/skills/x"),
        format!("Set-Content -LiteralPath '{home}/config.toml' -Value 'model = \"y\"'"),
        format!("cat {home}/config.toml"),
    ];
    for command in &commands {
        let input = json!({"command": command});
        assert_eq!(
            codex(&fx, Some(&home), "Bash", &input),
            home_refused(),
            "{command}"
        );
    }
}

/// A write to the home's `config.toml` through each spelling of the variable is refused.
#[test]
fn a_write_to_codex_homes_config_through_its_variable_is_refused() {
    let fx = Fixture::new("codex-home-var");
    let home = home(&fx);
    for command in [
        "echo 'model = \"y\"' > $CODEX_HOME/config.toml",
        "echo 'model = \"y\"' > \"${CODEX_HOME}/config.toml\"",
        "Set-Content -LiteralPath \"$env:CODEX_HOME\\config.toml\" -Value 'model = \"y\"'",
    ] {
        let input = json!({"command": command});
        assert_eq!(
            codex(&fx, Some(&home), "Bash", &input),
            home_refused(),
            "{command}"
        );
    }
}

/// A `cd` into the home, then a bare name: refused, in Bash's and PowerShell's forms.
#[test]
fn a_write_after_a_cd_into_codex_home_is_refused() {
    let fx = Fixture::new("codex-home-cd");
    let home = home(&fx);
    for command in [
        format!("cd '{home}' && echo 'model = \"y\"' > config.toml"),
        "cd ../codex-home && echo 'model = \"y\"' > config.toml".to_owned(),
        format!("Set-Location '{home}'; Set-Content -LiteralPath config.toml -Value 'x'"),
    ] {
        let input = json!({"command": command});
        assert_eq!(
            codex(&fx, Some(&home), "Bash", &input),
            home_refused(),
            "{command}"
        );
    }
}

/// Without `--codex-home` (Claude's entry, an older hook line, or a home the line could not
/// carry) the folder's path is not judged, and with it a write in the matter goes on.
#[test]
fn only_codex_home_is_refused_and_only_when_the_hook_names_it() {
    let fx = Fixture::new("codex-home-scope");
    let home = home(&fx);
    let into_home = json!({"command": patch(&format!("*** Add File: {home}/AGENTS.md\n+x"))});
    assert_eq!(codex(&fx, None, "apply_patch", &into_home), "");
    let in_matter = json!({"command": patch("*** Add File: notes.txt\n+x")});
    assert_eq!(codex(&fx, Some(&home), "apply_patch", &in_matter), "");
    let shell = json!({"command": "echo x > notes.txt"});
    assert_eq!(codex(&fx, Some(&home), "Bash", &shell), "");
}

/// Under Codex's hook a command whose text names the folder Codex's home sits in, or the
/// variable, is refused with `--codex-home` or without it; under Claude's hook (`--matter`) the
/// rule is not applied.
#[test]
fn codex_homes_folder_name_is_refused_under_codexs_hook_without_the_home_argument() {
    let fx = Fixture::new("codex-home-no-arg");
    for command in [
        "cat ~/AppData/Local/ai.clauseo.desktop/agents/Codex-ACP/home/config.toml",
        "echo x > \"$CODEX_HOME/AGENTS.md\"",
    ] {
        let input = json!({"command": command});
        assert_eq!(
            codex(&fx, None, "Bash", &input),
            home_refused(),
            "{command}"
        );
        let hook = fx.hook("Bash", &input).to_string();
        let matter = fx.path("m");
        let claude = guard_with(&["--matter", matter.as_str()], hook.as_bytes());
        assert_ne!(claude, home_refused(), "{command}");
    }
}

/// Codex's home where the app puts it, `<data>/agents/codex-acp/home`, under the fixture, as a
/// shell word (forward slashes), with `config.toml` in it; and the folder above it.
fn app_home(fx: &Fixture) -> (String, String) {
    let home = fx.path("data/agents/codex-acp/home");
    write::create_dir_all(Path::new(&home)).unwrap();
    write::write_file(&Path::new(&home).join("config.toml"), b"x = 1\n").unwrap();
    let home = home.replace('\\', "/");
    let parent = home.strip_suffix("/home").unwrap().to_owned();
    (home, parent)
}

/// Each command is refused under Codex's hook with `--codex-home <home>`.
fn assert_refused(fx: &Fixture, home: &str, commands: &[String]) {
    for command in commands {
        let input = json!({"command": command});
        assert_eq!(
            codex(fx, Some(home), "Bash", &input),
            home_refused(),
            "{command}"
        );
    }
}

/// A command that names the folder above the home, or the one above that, is refused: a copy,
/// an unpack, a clone or a removal there reaches the home.
#[test]
fn a_command_naming_the_folders_above_codex_home_is_refused() {
    let fx = Fixture::new("codex-home-above");
    let (home, parent) = app_home(&fx);
    let agents = parent.strip_suffix("/codex-acp").unwrap().to_owned();
    let commands = [
        format!("cp -R kit/. '{parent}'"),
        format!("tar -C '{parent}' -xf kit.tar"),
        format!("git -C '{parent}' clone https://example.org/x.git home"),
        format!("rm -rf '{parent}'"),
        format!("cp -R kit '{agents}'"),
    ];
    assert_refused(&fx, &home, &commands);
    let patch = json!({"command": patch(&format!("*** Add File: {parent}/home/AGENTS.md\n+x"))});
    assert_eq!(
        codex(&fx, Some(&home), "apply_patch", &patch),
        home_refused()
    );
}

/// A command whose text names the folder Codex's home sits in, in any case and with either
/// slash, is refused, whatever spelling leads there.
#[test]
fn a_command_naming_codex_homes_folder_anywhere_is_refused() {
    let fx = Fixture::new("codex-home-name");
    let (home, _) = app_home(&fx);
    let commands = [
        "cat ~/AppData/Local/ai.clauseo.desktop/agents/codex-acp/home/config.toml",
        "echo x > \"$HOME/Library/x/agents/codex-acp/home/AGENTS.md\"",
        "Set-Content \"$env:LOCALAPPDATA\\x\\agents\\codex-acp\\home\\AGENTS.md\" x",
        "python3 -c \"open(os.path.expanduser('~/d/agents/codex-acp/home/A.md'), 'w')\"",
        "ln -s /d/agents/codex-acp/home h && echo x > h/AGENTS.md",
        "copy x \\\\localhost\\C$\\Users\\a\\agents\\Codex-ACP\\home\\AGENTS.md",
        "copy x \\\\.\\C:\\d\\agents\\CODEX-ACP\\home\\AGENTS.md",
        "Set-Content Microsoft.PowerShell.Core\\FileSystem::C:\\d\\agents\\codex-acp\\home\\x 1",
    ]
    .map(str::to_owned);
    assert_refused(&fx, &home, &commands);
}

/// A tool run by its path in Codex's install (pasted from `which rg`) is refused, and the reason
/// tells the agent to run it by name: Codex puts that folder first on PATH, so `rg` runs.
#[test]
fn a_tool_run_by_its_path_in_codex_install_is_told_to_run_it_by_name() {
    let fx = Fixture::new("codex-home-tool-path");
    let (home, parent) = app_home(&fx);
    let commands = [
        format!("'{parent}/install/path/rg' -n Indemnity ."),
        format!("{home}/tmp/arg0/x/apply_patch"),
    ];
    assert_refused(&fx, &home, &commands);
    assert!(home_refused().contains("Run tools by name (for example rg)"));
}

/// The whole text is read, past the words the path layer looks at, and past many folder changes.
#[test]
fn a_write_to_codex_home_after_many_words_or_folder_changes_is_refused() {
    let fx = Fixture::new("codex-home-long");
    let (home, parent) = app_home(&fx);
    let filler = vec!["true"; 80].join(" && ");
    let cds = vec!["cd sub"; 16].join(" && ");
    let commands = [
        format!("{filler} && echo x > '{home}/config.toml'"),
        format!("{cds} && cd '{parent}' && cd home && echo x > config.toml"),
    ];
    assert_refused(&fx, &home, &commands);
}

/// The variable in cmd's `%…%` spelling is refused too.
#[test]
fn codex_homes_variable_in_cmds_spelling_is_refused() {
    let fx = Fixture::new("codex-home-cmd");
    let home = home(&fx);
    assert_refused(&fx, &home, &["type %CODEX_HOME%\\config.toml".to_owned()]);
}

/// Another letter case of the home's path names it where the file system ignores case.
#[cfg(any(target_os = "macos", windows))]
#[test]
fn codex_home_in_another_letter_case_is_refused() {
    let fx = Fixture::new("codex-home-case");
    let home = home(&fx);
    let upper = home.replace("codex-home", "CODEX-HOME");
    let patch = json!({"command": patch(&format!("*** Add File: {upper}/AGENTS.md\n+x"))});
    assert_eq!(
        codex(&fx, Some(&home), "apply_patch", &patch),
        home_refused()
    );
}

/// A link in the matter that leads into the home: a write through it is refused.
#[cfg(unix)]
#[test]
fn a_write_through_a_link_into_codex_home_is_refused() {
    let fx = Fixture::new("codex-home-link");
    let home = home(&fx);
    write::symlink(Path::new(&home), &fx.matter().join("h")).unwrap();
    let patch = json!({"command": patch("*** Add File: h/AGENTS.md\n+x")});
    assert_eq!(
        codex(&fx, Some(&home), "apply_patch", &patch),
        home_refused()
    );
    assert_refused(&fx, &home, &["echo x > h/config.toml".to_owned()]);
}
