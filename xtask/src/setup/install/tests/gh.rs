//! Tests for `install.rs`'s download through gh: when it is chosen over curl, its command, its
//! retries, and that nothing it prints could hold a token (xtask never reads one).

use super::*;

/// The file a gh command writes: the argument after `--output`.
pub(super) fn gh_output(cmd: &Cmd) -> &str {
    let at = cmd.args.iter().position(|arg| arg == "--output").unwrap();
    &cmd.args[at + 1]
}

/// The gh command that downloads `asset` of `b`.
fn expected_gh(b: &Binary, asset: &str) -> String {
    format!(
        "gh release download v{} --repo {} --pattern {asset} --output {asset} --clobber",
        b.version, b.repo
    )
}

fn with_gh(bin: &Path) -> Machine<'static> {
    Machine {
        fetcher: Fetcher::Gh,
        ..linux(bin, true)
    }
}

#[test]
fn gh_is_chosen_only_with_a_token_and_gh_on_path() {
    let dir = Scratch::new("setup-install-gh-choose");
    let with = dir.join("with");
    Scratch::create_dir(&with).unwrap();
    Scratch::write(with.join(GH_FILE), b"").unwrap();
    let without = dir.join("without");
    Scratch::create_dir(&without).unwrap();
    let path = |dirs: &[&Path]| std::env::join_paths(dirs).unwrap();
    assert_eq!(
        Fetcher::choose(true, &path(&[&without, &with])),
        Fetcher::Gh
    );
    assert_eq!(Fetcher::choose(true, &path(&[&without])), Fetcher::Curl);
    assert_eq!(Fetcher::choose(false, &path(&[&with])), Fetcher::Curl);
    assert_eq!(TOKEN_VARS, ["GH_TOKEN", "GITHUB_TOKEN"]);
}

#[test]
fn with_gh_the_download_is_one_gh_command_that_holds_no_token() {
    let dir = Scratch::new("setup-install-gh");
    let bin = dir.join("bin");
    let gitleaks = binary("gitleaks");
    let asset = expected_asset(gitleaks, "linux", "x86_64");
    let url = expected_url(gitleaks, &asset);
    let gh = gh_cmd(gitleaks, &asset);
    // No variable set for it: gh inherits the token from xtask's environment.
    assert_eq!(gh.env, Vec::<(String, String)>::new());
    assert_eq!(gh.to_string(), expected_gh(gitleaks, &asset));
    // The pins are the real ones, so the fake asset fails its sha256 check after the download.
    let (_, ran, printed) = install_with(gitleaks, &with_gh(&bin), ASSET_BYTES, &mut never);
    assert_eq!(ran[0].0, expected_gh(gitleaks, &asset));
    assert_eq!(ran.len(), 1, "{ran:?}");
    assert!(printed.starts_with(&format!(
        "installing gitleaks {}: downloading {url} through gh, authenticated (GH_TOKEN or \
         GITHUB_TOKEN is set)\n",
        gitleaks.version
    )));
}

#[test]
fn without_gh_the_download_is_anonymous_curl_as_before() {
    let dir = Scratch::new("setup-install-no-gh");
    let bin = dir.join("bin");
    let gitleaks = binary("gitleaks");
    let asset = expected_asset(gitleaks, "linux", "x86_64");
    let url = expected_url(gitleaks, &asset);
    let (_, ran, _) = install_with(gitleaks, &linux(&bin, true), ASSET_BYTES, &mut never);
    assert_eq!(ran[0].0, expected_curl(&asset, &url));
}

/// What `install_binary` prints and runs for gitleaks through gh when gh's attempts end as
/// `attempts` says (true for success); every sleep succeeds.
fn gh_attempts(case: &str, attempts: &[bool]) -> (Vec<String>, String) {
    let dir = Scratch::new(&format!("setup-install-gh-{case}"));
    let bin = dir.join("bin");
    let gitleaks = binary("gitleaks");
    let mut left = attempts.iter();
    let mut ran = Vec::new();
    let mut runner = |cmd: &Cmd, at: &Path, _: Output| -> io::Result<Ran> {
        ran.push(cmd.to_string());
        match cmd.program {
            Program::Named("gh") if *left.next().unwrap() => {
                Scratch::write(at.join(gh_output(cmd)), SWAPPED)?;
                exited(true, "", "")
            }
            Program::Named("gh") => exited(false, "", "HTTP 504: Gateway Timeout\n"),
            Program::Named("sleep") => exited(true, "", ""),
            _ => never(cmd),
        }
    };
    let mut out = Printer::new(Vec::new(), false);
    assert!(!install_binary(
        &mut out,
        gitleaks,
        &with_gh(&bin),
        &dir,
        &mut runner
    ));
    assert_eq!(files_in(&bin), []);
    (ran, String::from_utf8(out.into_inner()).unwrap())
}

#[test]
fn a_failed_gh_attempt_is_tried_again_after_10_then_30_seconds() {
    let gitleaks = binary("gitleaks");
    let asset = expected_asset(gitleaks, "linux", "x86_64");
    let url = expected_url(gitleaks, &asset);
    let gh = expected_gh(gitleaks, &asset);
    let (ran, printed) = gh_attempts("retried", &[false, false, true]);
    assert_eq!(ran, [&gh, "sleep 10", &gh, "sleep 30", &gh]);
    // The third attempt downloads SWAPPED, which fails the same sha256 check curl's would.
    let lines: Vec<&str> = printed.lines().collect();
    assert_eq!(
        lines[1..3],
        [
            format!(
                "downloading {url} through gh (attempt 1 of 3) failed: HTTP 504: Gateway \
                 Timeout; trying again in 10 s"
            ),
            format!(
                "downloading {url} through gh (attempt 2 of 3) failed: HTTP 504: Gateway \
                 Timeout; trying again in 30 s"
            ),
        ]
    );
    assert!(lines[3].starts_with(&format!(
        "{url} has sha256 {SWAPPED_SHA256}, but the pin is"
    )));
}

#[test]
fn three_failed_gh_attempts_fail_with_gh_as_the_fix_and_print_no_token() {
    let gitleaks = binary("gitleaks");
    let asset = expected_asset(gitleaks, "linux", "x86_64");
    let url = expected_url(gitleaks, &asset);
    let gh = expected_gh(gitleaks, &asset);
    let (ran, printed) = gh_attempts("fails", &[false, false, false]);
    assert_eq!(ran, [&gh, "sleep 10", &gh, "sleep 30", &gh]);
    assert_eq!(
        printed,
        format!(
            "installing gitleaks {v}: downloading {url} through gh, authenticated (GH_TOKEN or \
             GITHUB_TOKEN is set)\n\
             downloading {url} through gh (attempt 1 of 3) failed: HTTP 504: Gateway Timeout; \
             trying again in 10 s\n\
             downloading {url} through gh (attempt 2 of 3) failed: HTTP 504: Gateway Timeout; \
             trying again in 30 s\n\
             downloading {url} through gh (attempt 3 of 3) failed: HTTP 504: Gateway Timeout\n\
             \x20   fix: run `{gh}` by hand in an empty directory, with GH_TOKEN or GITHUB_TOKEN \
             set, and check its output, then run cargo xtask setup --binaries again; with neither \
             set, setup downloads {url} with curl instead\n",
            v = gitleaks.version
        )
    );
}
