//! The profile hook's session `PATH` folders (Clauseo's Python runtime): after stetdoc's, a
//! relative one skipped, and `PYTHONHOME` and `PYTHONPATH` dropped only with folders on.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::PathBuf;

use clauseo_settings::AgentLaunch;

use crate::agent::{PYTHON_VARS, launch_spec, session_path};

/// The profile hook's folders (`ProfileHook::agent_path`, Clauseo's Python runtime) go after
/// stetdoc's and before the merged `PATH`, in their order; a relative one is skipped, and only
/// when one of the hook's goes on do `PYTHONHOME` and `PYTHONPATH` go.
#[test]
fn the_hook_folders_go_between_stetdoc_and_the_merged_path() {
    let sep = if cfg!(windows) { ";" } else { ":" };
    let abs = |unix: &str, windows: &str| PathBuf::from(if cfg!(windows) { windows } else { unix });
    let stetdoc = abs("/app/stetdoc", r"C:\stetdoc");
    let bin = abs("/rt/bin", r"C:\rt\clauseo-bin");
    let root = abs("/rt", r"C:\rt");
    // Stetdoc first, the hook's in order, a relative one skipped.
    let (path, hook) = session_path(
        Some(&stetdoc),
        vec![bin.clone(), PathBuf::from("rt/bin"), root.clone()],
    );
    assert_eq!(path, [stetdoc.clone(), bin.clone(), root.clone()]);
    assert!(hook);
    // Only relative ones, or none: stetdoc alone, and the Python variables stay.
    assert_eq!(
        session_path(Some(&stetdoc), vec![PathBuf::from("bin")]),
        (vec![stetdoc.clone()], false)
    );
    assert_eq!(
        session_path(Some(&stetdoc), Vec::new()),
        (vec![stetdoc.clone()], false)
    );
    assert_eq!(
        session_path(None, vec![bin.clone()]),
        (vec![bin.clone()], true)
    );
    let agent = AgentLaunch {
        command: std::env::temp_dir()
            .join("agent")
            .to_str()
            .unwrap()
            .to_owned(),
        args: Vec::new(),
        env: BTreeMap::new(),
    };
    let run = |hook: Vec<PathBuf>| {
        let (path, dropping) = session_path(Some(&stetdoc), hook);
        let dropped: Vec<&str> = if dropping {
            PYTHON_VARS.to_vec()
        } else {
            Vec::new()
        };
        let inherited = [
            ("Path", "/usr/bin"),
            ("PYTHONHOME", "/elsewhere"),
            ("PYTHONPATH", "/elsewhere/lib"),
        ];
        let spec = launch_spec(
            &agent,
            BTreeMap::new(),
            BTreeMap::new(),
            &dropped,
            inherited
                .into_iter()
                .map(|(name, value)| (OsString::from(name), OsString::from(value))),
            std::env::temp_dir(),
            &path,
        )
        .unwrap();
        let get = |name: &str| {
            spec.env()
                .get(&OsString::from(name))
                .map(|value| value.to_str().unwrap().to_owned())
        };
        (get("Path"), get("PYTHONHOME"), get("PYTHONPATH"))
    };
    let (first, bin_s, root_s) = (
        stetdoc.to_str().unwrap(),
        bin.to_str().unwrap(),
        root.to_str().unwrap(),
    );
    assert_eq!(
        run(vec![bin.clone(), root.clone()]),
        (
            Some(format!("{first}{sep}{bin_s}{sep}{root_s}{sep}/usr/bin")),
            None,
            None
        )
    );
    // An empty hook: the PATH as before, and both variables inherited.
    assert_eq!(
        run(Vec::new()),
        (
            Some(format!("{first}{sep}/usr/bin")),
            Some("/elsewhere".to_owned()),
            Some("/elsewhere/lib".to_owned())
        )
    );
}
