//! The harness's manifest, `<root>\s15-manifest.txt`: what prepare found and made, so the rows
//! know the pool users and folders and cleanup deletes exactly what was made. One `key<TAB>value`
//! per line (Windows paths hold no tab); rewritten whole after each change.

use std::path::{Path, PathBuf};

use super::sys::write_file;

/// The manifest's file name in the harness root.
pub(crate) const MANIFEST: &str = "s15-manifest.txt";
/// The session key of matter A's second pool user: a pool user serves one live session at a
/// time, so a second session of matter A (`pipes-same-matter`), and control (a) while A's
/// session runs, take this one. It is granted matter A's folder and has a state folder of its own.
pub(crate) const SECOND_A: &str = "A2";

/// A pool user the harness runs as.
#[derive(Debug, Clone, Default)]
pub(crate) struct Pool {
    /// The SID.
    pub(crate) sid: String,
    /// `ClauseoSbNN`.
    pub(crate) name: String,
}

/// What prepare recorded.
#[derive(Debug, Clone, Default)]
pub(crate) struct Manifest {
    /// The harness root.
    pub(crate) root: PathBuf,
    /// The data folder that holds `sandbox\` and `secrets.bin`.
    pub(crate) data_dir: PathBuf,
    /// Whether prepare ran `clauseo-sandbox-setup --set-up`.
    pub(crate) setup_ran: bool,
    /// The set-up exe prepare used, for cleanup's `--remove`.
    pub(crate) setup_exe: Option<PathBuf>,
    /// Matter A's pool user, matter B's, then matter A's second ([`SECOND_A`]).
    pub(crate) pools: Vec<Pool>,
    /// `node.exe` in the bin folder.
    pub(crate) node: Option<PathBuf>,
    /// `codex.exe` in the bin folder.
    pub(crate) codex: Option<PathBuf>,
    /// Codex's `path` folder (its `rg.exe`) in the bin folder, when the package has one.
    pub(crate) codex_path: Option<PathBuf>,
    /// The Claude SDK's `claude.exe` in the bin folder (the emulation experiment, `emu.rs`).
    pub(crate) claude: Option<PathBuf>,
    /// Folders made, in the order made.
    pub(crate) dirs: Vec<PathBuf>,
    /// Files made outside those folders, in the order made.
    pub(crate) files: Vec<PathBuf>,
    /// Global atoms the `atoms` row added and hasn't deleted yet.
    pub(crate) atoms: Vec<String>,
}

impl Manifest {
    /// The manifest file of the harness root `root`.
    pub(crate) fn path_in(root: &Path) -> PathBuf {
        root.join(MANIFEST)
    }

    /// The bin folder.
    pub(crate) fn bin(&self) -> PathBuf {
        self.root.join("bin")
    }

    /// The harness exe's copy in bin.
    pub(crate) fn harness_exe(&self) -> PathBuf {
        self.bin().join("s15_launch_vm.exe")
    }

    /// The runner's copy in bin.
    pub(crate) fn runner_exe(&self) -> PathBuf {
        self.bin()
            .join(clauseo_sandbox::windows::runner::RUNNER_EXE)
    }

    /// The logs folder.
    pub(crate) fn logs(&self) -> PathBuf {
        self.root.join("logs")
    }

    /// Matter `which`'s (`A` or `B`) matter folder; [`SECOND_A`]'s is matter A's.
    pub(crate) fn matter(&self, which: &str) -> PathBuf {
        let which = if which == SECOND_A { "A" } else { which };
        self.root.join("matters").join(which).join("matter")
    }

    /// Session key `which`'s state folder (`A`, `B` or [`SECOND_A`], each its own).
    pub(crate) fn state(&self, which: &str) -> PathBuf {
        self.root.join("matters").join(which).join("state")
    }

    /// Reads the manifest in `root`; `None` when there is none.
    pub(crate) fn load(root: &Path) -> std::io::Result<Option<Manifest>> {
        let text = match std::fs::read_to_string(Manifest::path_in(root)) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        let mut manifest = Manifest {
            root: root.to_path_buf(),
            ..Manifest::default()
        };
        for line in text.lines() {
            let Some((key, value)) = line.split_once('\t') else {
                continue;
            };
            let path = PathBuf::from(value);
            match key {
                "data" => manifest.data_dir = path,
                "setup-ran" => manifest.setup_ran = value == "yes",
                "setup-exe" => manifest.setup_exe = Some(path),
                "node" => manifest.node = Some(path),
                "codex" => manifest.codex = Some(path),
                "codex-path" => manifest.codex_path = Some(path),
                "claude" => manifest.claude = Some(path),
                "dir" => manifest.dirs.push(path),
                "file" => manifest.files.push(path),
                "atom" => manifest.atoms.push(value.to_owned()),
                "pool" => {
                    if let Some((sid, name)) = value.split_once(' ') {
                        manifest.pools.push(Pool {
                            sid: sid.to_owned(),
                            name: name.to_owned(),
                        });
                    }
                }
                _ => {}
            }
        }
        Ok(Some(manifest))
    }

    /// Writes the manifest into its root.
    pub(crate) fn save(&self) -> std::io::Result<()> {
        let mut text = String::new();
        let mut put = |key: &str, value: &str| {
            text.push_str(key);
            text.push('\t');
            text.push_str(value);
            text.push('\n');
        };
        put("data", &self.data_dir.display().to_string());
        put("setup-ran", if self.setup_ran { "yes" } else { "no" });
        for (key, value) in [
            ("setup-exe", &self.setup_exe),
            ("node", &self.node),
            ("codex", &self.codex),
            ("codex-path", &self.codex_path),
            ("claude", &self.claude),
        ] {
            if let Some(value) = value {
                put(key, &value.display().to_string());
            }
        }
        for pool in &self.pools {
            put("pool", &format!("{} {}", pool.sid, pool.name));
        }
        for dir in &self.dirs {
            put("dir", &dir.display().to_string());
        }
        for file in &self.files {
            put("file", &file.display().to_string());
        }
        for atom in &self.atoms {
            put("atom", atom);
        }
        write_file(&Manifest::path_in(&self.root), text.as_bytes())
    }

    /// Loads the manifest in `root` from disk, applies `change`, and writes it back: for a row
    /// that records what it makes while it runs.
    pub(crate) fn update(root: &Path, change: impl FnOnce(&mut Manifest)) -> Result<(), String> {
        let mut on_disk = Manifest::load(root)
            .map_err(|e| e.to_string())?
            .ok_or("the manifest is gone")?;
        change(&mut on_disk);
        on_disk
            .save()
            .map_err(|e| format!("writing the manifest: {e}"))
    }

    /// Matter A's pool user and matter B's, when prepare found two.
    pub(crate) fn pool_pair(&self) -> Option<(&Pool, &Pool)> {
        Some((self.pools.first()?, self.pools.get(1)?))
    }

    /// Matter A's second pool user ([`SECOND_A`]), when prepare found three.
    pub(crate) fn second_a(&self) -> Option<&Pool> {
        self.pools.get(2)
    }
}
