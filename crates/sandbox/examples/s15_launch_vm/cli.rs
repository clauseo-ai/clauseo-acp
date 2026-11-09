//! The command line:
//!
//! ```text
//! s15_launch_vm prepare|rows|cleanup|row <name> [options]
//! s15_launch_vm --unattended [options]
//! s15_launch_vm probe <what> ...          (what the runner starts inside the sandbox)
//! options: --root <dir> --data-dir <dir> --node <node.exe> --codex <package dir or codex.exe>
//!          --claude <claude.exe, cli.js or a folder holding one>
//!          --runner <clauseo-sandbox-run.exe> --setup <clauseo-sandbox-setup.exe>
//!          --row-timeout <seconds> --interactive-desktop
//! ```
//!
//! `--interactive-desktop` (rows, a row, or unattended, in a console or RDP session, not session
//! 0): `node-pipes` needs each UI effect (desktop create, `SwitchDesktop`, clipboard write)
//! carried by a control and refused to the agent, else it fails (`gate.rs`).

use std::path::PathBuf;
use std::time::Duration;

/// The step to run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Step {
    /// Make the root, copy the programs, set up or find the pool, make and grant the matters.
    Prepare,
    /// One row by name.
    Row(String),
    /// Every row.
    Rows,
    /// Delete what the manifest lists.
    Cleanup,
    /// Prepare, every row, cleanup.
    Unattended,
}

/// The options.
#[derive(Debug, Clone)]
pub(crate) struct Opts {
    /// The step.
    pub(crate) step: Step,
    /// The harness root.
    pub(crate) root: PathBuf,
    /// `--data-dir`.
    pub(crate) data_dir: Option<PathBuf>,
    /// `--node`.
    pub(crate) node: Option<PathBuf>,
    /// `--codex`.
    pub(crate) codex: Option<PathBuf>,
    /// `--claude`.
    pub(crate) claude: Option<PathBuf>,
    /// `--runner`.
    pub(crate) runner: Option<PathBuf>,
    /// `--setup`.
    pub(crate) setup: Option<PathBuf>,
    /// The hard limit on each row, and on the set-up.
    pub(crate) row_timeout: Duration,
    /// `--interactive-desktop`: the strict UI gate for `node-pipes`.
    pub(crate) interactive_desktop: bool,
}

/// The default harness root.
pub(crate) const DEFAULT_ROOT: &str = r"C:\ClauseoS15Vm";
/// The default limit on each row.
const DEFAULT_ROW_TIMEOUT: u64 = 120;

/// The usage line.
pub(crate) const USAGE: &str = "usage: s15_launch_vm prepare|rows|cleanup|row <name> | \
     --unattended [--root D] [--data-dir D] [--node P] [--codex P] [--claude P] [--runner P] [--setup P] \
     [--row-timeout S] [--interactive-desktop] | probe <what> ...";

/// Parses the arguments after the program's name (not `probe`).
pub(crate) fn parse(args: &[String]) -> Result<Opts, String> {
    let mut step = None;
    let mut opts = Opts {
        step: Step::Rows,
        root: PathBuf::from(DEFAULT_ROOT),
        data_dir: None,
        node: None,
        codex: None,
        claude: None,
        runner: None,
        setup: None,
        row_timeout: Duration::from_secs(DEFAULT_ROW_TIMEOUT),
        interactive_desktop: false,
    };
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        let mut value = |flag: &str| {
            args.next()
                .cloned()
                .ok_or_else(|| format!("{flag} needs a value"))
        };
        match arg.as_str() {
            "--unattended" => set(&mut step, Step::Unattended)?,
            "--interactive-desktop" => opts.interactive_desktop = true,
            "--root" => opts.root = PathBuf::from(value(arg)?),
            "--data-dir" => opts.data_dir = Some(PathBuf::from(value(arg)?)),
            "--node" => opts.node = Some(PathBuf::from(value(arg)?)),
            "--codex" => opts.codex = Some(PathBuf::from(value(arg)?)),
            "--claude" => opts.claude = Some(PathBuf::from(value(arg)?)),
            "--runner" => opts.runner = Some(PathBuf::from(value(arg)?)),
            "--setup" => opts.setup = Some(PathBuf::from(value(arg)?)),
            "--row-timeout" => {
                let text = value(arg)?;
                let seconds = text
                    .parse::<u64>()
                    .map_err(|_not_a_number| format!("--row-timeout {text:?} isn't seconds"))?;
                opts.row_timeout = Duration::from_secs(seconds.max(1));
            }
            "prepare" => set(&mut step, Step::Prepare)?,
            "rows" => set(&mut step, Step::Rows)?,
            "cleanup" => set(&mut step, Step::Cleanup)?,
            "row" => set(&mut step, Step::Row(value(arg)?))?,
            other => return Err(format!("unknown argument {other:?}")),
        }
    }
    opts.step = step.ok_or_else(|| "no step given".to_owned())?;
    Ok(opts)
}

fn set(slot: &mut Option<Step>, step: Step) -> Result<(), String> {
    if slot.is_some() {
        return Err("give one step".to_owned());
    }
    *slot = Some(step);
    Ok(())
}
